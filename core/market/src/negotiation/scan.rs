use std::borrow::Cow;
use std::cmp::{max, min};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use actix_web::web::Data;
use chrono::{NaiveDateTime, Utc};
use parking_lot::Mutex;
use serde::Serialize;
use serde_bytes::ByteBuf;
use tokio::sync::{watch, Mutex as AsyncMutex};
use ya_client::model::market::scan::{NewScan, ScanType};
use ya_client::model::market::Offer;
use ya_client::model::NodeId;

use tracing::{event, Level};
use ya_core_model::net;
use ya_core_model::net::RemoteEndpoint;
use ya_market_resolver::flatten::flatten_properties;
use ya_market_resolver::resolver::expression::{build_expression, ResolveResult};
use ya_market_resolver::resolver::{ldap_parser, Expression, PropertySet};
use ya_persistence::executor::DbMixedExecutor;
use ya_service_bus::timeout::IntoTimeoutFuture;

use crate::db::dao::OfferDao;
use crate::protocol::discovery::message::{get_offers_addr, QueryOffers, RetrieveOffers};
use crate::testing::SubscriptionId;
use ya_core_model::market as market_model;

use super::error::ScanError;

// Default timeout
const REMOTE_CALL_TIMEOUT: Option<Duration> = Some(Duration::from_secs(30));
const MAX_OFFERS_PER_QUERY: usize = 10;

#[derive(Hash, Eq, PartialEq, Serialize, Clone)]
#[serde(transparent)]
pub struct ScanId {
    #[serde(serialize_with = "ser_scan")]
    scan_id: u64,
}

impl FromStr for ScanId {
    type Err = ScanError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let scan_id = s.parse().map_err(|_| ScanError::NotFound {
            scan_id: s.to_string(),
        })?;

        Ok(Self { scan_id })
    }
}

impl Display for ScanId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.scan_id, f)
    }
}

fn ser_scan<S>(scan_id: &u64, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    s.serialize_str(&scan_id.to_string())
}

struct Scanner {
    id: u64,
    owner: NodeId,
    timeout: Instant,
    timeout_extend: Duration,
    scan_type: ScanType,
    constraints: Option<Expression>,
    constraints_raw: Option<String>,
    last_ts: Option<NaiveDateTime>,
    direct: HashMap<NodeId, Arc<AsyncMutex<DirectState>>>,
}

//
#[derive(Default)]
struct DirectState {
    iterator: Option<ByteBuf>,
    offers: Vec<SubscriptionId>,
}

impl Scanner {
    fn new(id: u64, owner: NodeId, new_scan: NewScan) -> Result<Self, ScanError> {
        let timeout_extend = Duration::from_secs(new_scan.timeout.unwrap_or(300));
        let timeout = Instant::now() + timeout_extend;
        let scan_type = new_scan.scan_type;
        let last_ts = None;

        let constraints = if let Some(constraints) = new_scan.constraints.as_ref() {
            let tags = ldap_parser::parse(constraints)
                .map_err(|reason| ScanError::InvalidConstraint { reason })?;
            Some(
                build_expression(&tags).map_err(|e| ScanError::InvalidConstraint {
                    reason: e.to_string(),
                })?,
            )
        } else {
            None
        };
        let constraints_raw = new_scan.constraints;
        let direct = Default::default();

        Ok(Scanner {
            id,
            owner,
            timeout,
            timeout_extend,
            scan_type,
            constraints,
            constraints_raw,
            last_ts,
            direct,
        })
    }

    pub fn touch(&mut self) {
        self.timeout = max(self.timeout, Instant::now() + self.timeout_extend);
    }

    fn is_alive(slot: &Arc<AsyncMutex<Self>>) -> bool {
        if Arc::strong_count(slot) > 1 {
            return true;
        }
        let Ok(me) = slot.try_lock() else { return true };
        me.timeout > Instant::now()
    }

    pub async fn next(
        &mut self,
        dao: &OfferDao<'_>,
        max_items: u64,
    ) -> Result<Option<Vec<Offer>>, ScanError> {
        // for now we do not trace demands
        if matches!(self.scan_type, ScanType::Demand) {
            log::warn!("scan for Demand");
            return Ok(None);
        }

        let offers = dao
            .get_scan_offers(self.last_ts, Utc::now().naive_utc(), Some(max_items as i64))
            .await
            .map_err(|cause| ScanError::InternalDbError {
                context: Cow::Borrowed("Failed to get offers"),
                cause,
            })?;
        let max_ts = offers.iter().filter_map(|offer| offer.insertion_ts).max();

        if let Some(max_ts) = max_ts {
            let offers = offers
                .into_iter()
                .filter_map(|o| {
                    if let Some(constraints) = &self.constraints {
                        let props = flatten_properties(&o.properties).ok()?;
                        let property_set = PropertySet::from_flat_props(&props);
                        if matches!(constraints.resolve(&property_set), ResolveResult::True) {
                            o.into_client_offer().ok()
                        } else {
                            None
                        }
                    } else {
                        o.into_client_offer().ok()
                    }
                })
                .collect();

            self.last_ts = Some(max_ts);
            Ok(Some(offers))
        } else {
            Ok(None)
        }
    }
}

impl Drop for Scanner {
    fn drop(&mut self) {
        event!(
            Level::INFO,
            entity = "scan",
            action = "drop",
            scan_id = self.id,
            "Scan dropped"
        );
    }
}

#[derive(Clone)]
pub struct LastChange {
    watch: watch::Sender<Instant>,
}

impl LastChange {
    pub fn new() -> Self {
        let (watch, _) = watch::channel(Instant::now());
        Self { watch }
    }

    pub fn subscribe(&self) -> watch::Receiver<Instant> {
        let rx = self.watch.subscribe();
        log::debug!("active scanners: {}", self.watch.receiver_count());
        rx
    }

    pub fn notify(&self) {
        self.watch.send(Instant::now()).ok();
    }
}

pub struct ScannerSet {
    scanners: Mutex<HashMap<ScanId, Arc<AsyncMutex<Scanner>>>>,
    seq_no: AtomicU64,
    db: DbMixedExecutor,
    watch: LastChange,
}

impl ScannerSet {
    pub fn new(db: DbMixedExecutor) -> Data<Self> {
        let scanners = Default::default();
        let seq_no = AtomicU64::new(0);
        let watch = LastChange::new();
        let me = Data::new(Self {
            db,
            scanners,
            seq_no,
            watch,
        });

        {
            let me = me.clone();
            tokio::task::spawn_local(async move {
                let mut it = tokio::time::interval(Duration::from_secs(60));
                loop {
                    let _ = it.tick().await;
                    me.clean();
                }
            });
        }

        me
    }

    pub fn begin(&self, owner_id: NodeId, new_scan: NewScan) -> Result<ScanId, ScanError> {
        let scan_id = self.seq_no.fetch_add(1, Ordering::AcqRel);
        let scanner = Scanner::new(scan_id, owner_id, new_scan)?;

        self.scanners
            .lock()
            .insert(ScanId { scan_id }, Arc::new(AsyncMutex::new(scanner)));
        event!(
            Level::INFO,
            entity = "scan",
            action = "created",
            scan_id = scan_id,
            "Scan created"
        );
        Ok(ScanId { scan_id })
    }

    fn get_scan(&self, scan_id: &ScanId) -> Result<Arc<AsyncMutex<Scanner>>, ScanError> {
        self.scanners
            .lock()
            .get(scan_id)
            .cloned()
            .ok_or_else(|| ScanError::NotFound {
                scan_id: scan_id.scan_id.to_string(),
            })
    }

    fn subscribe(&self) -> watch::Receiver<Instant> {
        self.watch.subscribe()
    }

    /// Gets the LastChange instance for synchronization
    pub fn offers_watch(&self) -> &LastChange {
        &self.watch
    }

    fn clean(&self) {
        let mut g = self.scanners.lock();
        let prev_len = g.len();
        g.retain(|_, slot| Scanner::is_alive(slot));
        let n = prev_len - g.len();
        if n > 0 {
            log::debug!("clean out {n} scanners");
        }
    }

    pub async fn direct_offers(
        &self,
        owner_id: NodeId,
        scan_id: ScanId,
        peer_id: NodeId,
        max_items: u64,
    ) -> Result<Vec<Offer>, ScanError> {
        let scan = self.get_scan(&scan_id)?;
        let mut g = scan.lock().await;
        if owner_id != g.owner {
            return Err(ScanError::Forbidden);
        }
        let constraint_expr = g.constraints_raw.clone();
        g.touch();
        let ctx = {
            let ctx = g.direct.entry(peer_id).or_insert_with(|| {
                Arc::new(AsyncMutex::new(DirectState {
                    iterator: None,
                    offers: Default::default(),
                }))
            });
            Arc::clone(ctx)
        };
        drop(g);
        let offers_addr = get_offers_addr(market_model::BUS_ID);

        let mut g_ctx = ctx.lock().await;
        if g_ctx.offers.is_empty() {
            let iterator = g_ctx.iterator.take();

            let query = QueryOffers {
                node_id: Some(peer_id),
                constraint_expr,
                iterator,
            };
            let result = net::from(owner_id)
                .to(peer_id)
                .service(&offers_addr)
                .call(query)
                .timeout(REMOTE_CALL_TIMEOUT)
                .await
                .map_err(|_| ScanError::FetchTimeout)?
                .map_err(|e| match e {
                    ya_service_bus::Error::GsbBadRequest(msg)
                        if msg.ends_with("endpoint address not found") =>
                    {
                        ScanError::OldPeer
                    }
                    e => e.into(),
                })?
                .map_err(|cause| ScanError::DiscoveryRemoteError {
                    operation: Cow::Borrowed("query peer offers ids"),
                    cause,
                })?;

            g_ctx.iterator = result.iterator;
            g_ctx.offers.extend(result.offers.into_iter())
        }

        let query_size = min(
            max(max_items as usize, MAX_OFFERS_PER_QUERY),
            g_ctx.offers.len(),
        );
        let offer_ids = g_ctx.offers.drain(..query_size).collect();

        let result = net::from(owner_id)
            .to(peer_id)
            .service(&offers_addr)
            .call(RetrieveOffers { offer_ids })
            .timeout(REMOTE_CALL_TIMEOUT)
            .await
            .map_err(|_| ScanError::FetchTimeout)??
            .map_err(|cause| ScanError::DiscoveryRemoteError {
                operation: Cow::Borrowed("retrieving offer details"),
                cause,
            })?;

        Ok(result
            .into_iter()
            .filter_map(|o| o.into_client_offer().ok())
            .collect::<Vec<_>>())
    }

    pub async fn collect(
        &self,
        owner_id: NodeId,
        scan_id: ScanId,
        max_items: u64,
    ) -> Result<Vec<Offer>, ScanError> {
        let mut wait = self.subscribe();
        let scan = self.get_scan(&scan_id)?;
        let mut g = scan.lock().await;
        if owner_id != g.owner {
            return Err(ScanError::Forbidden);
        }
        g.touch();
        drop(g);

        if max_items == 0 {
            return Ok(Vec::new());
        }

        loop {
            let mut g = scan.lock().await;
            let dao = self.db.as_dao::<OfferDao>();
            while let Some(offers) = g.next(&dao, max_items).await? {
                if !offers.is_empty() {
                    g.touch();
                    return Ok(offers);
                }
            }
            drop(g);
            if let Err(_e) = wait.changed().await {
                return Err(ScanError::Gone {
                    scan_id: scan_id.scan_id,
                });
            }
        }
    }

    pub async fn end(&self, owner_id: NodeId, scan_id: ScanId) -> Result<(), ScanError> {
        let scan = self.get_scan(&scan_id)?;
        let g = scan.lock().await;
        if owner_id != g.owner {
            return Err(ScanError::Forbidden);
        }
        drop(g);
        self.scanners.lock().remove(&scan_id);
        event!(
            Level::INFO,
            entity = "scan",
            action = "removed",
            scan_id = display(&scan_id),
            "Scan removed"
        );
        Ok(())
    }

    pub fn notify(&self) {
        self.watch.notify();
    }
}
