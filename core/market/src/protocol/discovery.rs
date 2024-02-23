//! Discovery protocol interface
use futures::TryFutureExt;
use metrics::{counter, timing, value};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;

use ya_client::model::NodeId;
use ya_core_model::market::BUS_ID;
use ya_core_model::net::local::{BroadcastMessage, SendBroadcastMessage};
use ya_net::{self as net, RemoteEndpoint};
use ya_service_bus::timeout::{IntoDuration, IntoTimeoutFuture};
use ya_service_bus::typed::ServiceBinder;
use ya_service_bus::{Error as BusError, RpcEndpoint, RpcMessage};

use super::callback::HandlerSlot;
use crate::config::DiscoveryConfig;
use crate::db::model::{Offer as ModelOffer, SubscriptionId};
use crate::identity::{IdentityApi, IdentityError};
use parking_lot::Mutex as PlMutex;

pub mod builder;
pub mod error;
pub mod message;

use crate::PROTOCOL_VERSION;
use error::*;
use message::*;

const MAX_OFFER_IDS_PER_BROADCAST: usize = 8;

/// Responsible for communication with markets on other nodes
/// during discovery phase.
#[derive(Clone)]
pub struct Discovery {
    inner: Arc<DiscoveryImpl>,
}

pub(super) struct OfferHandlers {
    filter_out_known_ids: HandlerSlot<OffersBcast>,
    receive_remote_offers: HandlerSlot<OffersRetrieved>,
    get_local_offers_handler: HandlerSlot<RetrieveOffers>,
    offer_unsubscribe_handler: HandlerSlot<UnsubscribedOffersBcast>,
}

pub struct DiscoveryImpl {
    identity: Arc<dyn IdentityApi>,

    /// Sending queues.
    offer_sending_queue: Mutex<Vec<SubscriptionId>>,
    unsub_sending_queue: Mutex<Vec<SubscriptionId>>,
    lazy_binder_prefix: Mutex<Option<String>>,

    /// Receiving queue.
    offers_receiving_queue: mpsc::Sender<(NodeId, OffersBcast)>,
    offer_handlers: OfferHandlers,

    config: DiscoveryConfig,
    /// We need this to determine, if we use hybrid NET. Should be removed together
    /// with central NET implementation in future.
    net_type: net::NetType,
    ban_cache: BanCache,
}

struct BanCache {
    inner: Arc<PlMutex<BanCacheInner>>,
}

struct BanCacheInner {
    banned_nodes: HashSet<NodeId>,
    ts: Instant,
    max_ban_time: std::time::Duration,
}

impl BanCache {
    fn new(max_ban_time: std::time::Duration) -> Self {
        let banned_nodes = Default::default();
        let ts = Instant::now();

        Self {
            inner: Arc::new(PlMutex::new(BanCacheInner {
                banned_nodes,
                ts,
                max_ban_time,
            })),
        }
    }

    fn is_banned_node(&self, node_id: &NodeId) -> bool {
        let mut g = self.inner.lock();
        if g.banned_nodes.contains(node_id) {
            if g.ts.elapsed() > g.max_ban_time {
                g.banned_nodes.clear();
                g.ts = Instant::now();
                false
            } else {
                true
            }
        } else {
            false
        }
    }

    fn ban_node(&self, node_id: NodeId) {
        let mut g = self.inner.lock();
        if g.banned_nodes.is_empty() {
            g.ts = Instant::now();
        }
        g.banned_nodes.insert(node_id);
    }
}

impl Discovery {
    #[inline]
    pub fn re_broadcast_enabled(&self) -> bool {
        self.is_hybrid_net()
    }

    pub fn is_hybrid_net(&self) -> bool {
        self.inner.net_type == net::NetType::Hybrid
    }

    pub async fn bcast_offers(&self, offer_ids: Vec<SubscriptionId>) -> Result<(), DiscoveryError> {
        if offer_ids.is_empty() {
            return Ok(());
        }
        // When there are 0 items in the queue we should schedule a send job.
        let must_schedule = {
            let mut queue = self.inner.offer_sending_queue.lock().await;
            let result = queue.len() == 0;

            queue.append(&mut offer_ids.clone());
            result
        };
        log::trace!(
            "bcast_offers done appending {} offers. must_schedule={}",
            offer_ids.len(),
            must_schedule
        );

        if must_schedule {
            let myself = self.clone();
            tokio::task::spawn_local(async move {
                // Sleep to collect multiple offers to send
                sleep(myself.inner.config.offer_broadcast_delay).await;
                myself.send_bcast_offers().await;
            });
        }
        Ok(())
    }

    /// Broadcasts Offers to other nodes in network. Connected nodes will
    /// get call to function bound at `OfferBcast`.
    async fn send_bcast_offers(&self) {
        // `...offer_queue` MUST be empty to trigger the sending again
        let offer_ids: Vec<SubscriptionId> =
            std::mem::take(&mut *self.inner.offer_sending_queue.lock().await);

        // Should never happen, but just to be certain.
        if offer_ids.is_empty() {
            return;
        }

        let default_id = match self.default_identity().await {
            Ok(id) => id,
            Err(e) => {
                log::error!(
                    "Error getting default identity, not sending bcast. error={:?}",
                    e
                );
                return;
            }
        };
        let size = offer_ids.len();
        log::debug!("Broadcasting offers. count={}", size);

        counter!("market.offers.broadcasts.net", 1);
        value!("market.offers.broadcasts.len", size as u64);

        if self.is_hybrid_net() {
            let mut iter = offer_ids.into_iter().peekable();
            while iter.peek().is_some() {
                let chunk = iter.by_ref().take(MAX_OFFER_IDS_PER_BROADCAST).collect();
                broadcast_offers(default_id, chunk).await;

                // Spread broadcasts into longer time frame. This way we avoid dropping Offers
                // on the other side and reduce peak network usage.
                tokio::time::sleep(self.inner.config.bcast_tile_time_margin).await;
            }
        } else {
            broadcast_offers(default_id, offer_ids).await;
        }
    }

    /// Ask remote Node for specified Offers.
    pub async fn get_remote_offers(
        &self,
        target_node_id: String,
        offer_ids: Vec<SubscriptionId>,
        timeout: impl IntoDuration,
    ) -> Result<Vec<ModelOffer>, DiscoveryError> {
        let target_node = NodeId::from_str(&target_node_id)
            .map_err(|e| DiscoveryError::InternalError(e.to_string()))?;

        Ok(net::from(self.default_identity().await?)
            .to(target_node)
            .service(&get_offers_addr(BUS_ID))
            .send(RetrieveOffers { offer_ids })
            .timeout(Some(timeout))
            .map_err(|_| {
                DiscoveryError::GsbError(
                    BusError::Timeout(format!(
                        "{}/{}",
                        get_offers_addr(BUS_ID),
                        RetrieveOffers::ID
                    ))
                    .to_string(),
                )
            })
            .await???)
    }

    pub async fn bcast_unsubscribes(
        &self,
        offer_ids: Vec<SubscriptionId>,
    ) -> Result<(), DiscoveryError> {
        if offer_ids.is_empty() {
            return Ok(());
        }

        // When there are 0 items in the queue we should schedule a send job.
        let must_schedule = {
            let mut queue = self.inner.unsub_sending_queue.lock().await;
            let result = queue.len() == 0;

            queue.append(&mut offer_ids.clone());
            result
        };

        log::trace!(
            "bcast_unsubscribes done appending {} offers. must_schedule={}",
            offer_ids.len(),
            must_schedule
        );

        if must_schedule {
            let myself = self.clone();
            tokio::task::spawn_local(async move {
                // Sleep to collect multiple unsubscribes to send
                sleep(myself.inner.config.unsub_broadcast_delay).await;
                myself.send_bcast_unsubscribes().await;
            });
        }
        Ok(())
    }

    async fn send_bcast_unsubscribes(&self) {
        // `...unsub_queue` MUST be empty to trigger the sending again
        let offer_ids: Vec<SubscriptionId> = self
            .inner
            .unsub_sending_queue
            .lock()
            .await
            .drain(..)
            .collect();

        // Should never happen, but just to be certain.
        if offer_ids.is_empty() {
            return;
        }
        let default_id = match self.default_identity().await {
            Ok(id) => id,
            Err(e) => {
                log::error!(
                    "Error getting default identity, not sending bcast. error={:?}",
                    e
                );
                return;
            }
        };

        let size = offer_ids.len();
        log::debug!("Broadcasting unsubscribes. count={}", size);
        counter!("market.offers.unsubscribes.broadcasts.net", 1);
        value!("market.offers.unsubscribes.broadcasts.len", size as u64);

        if self.is_hybrid_net() {
            let mut iter = offer_ids.into_iter().peekable();
            while iter.peek().is_some() {
                let chunk = iter.by_ref().take(MAX_OFFER_IDS_PER_BROADCAST).collect();
                broadcast_unsubscribed(default_id, chunk).await;
            }
        } else {
            broadcast_unsubscribed(default_id, offer_ids).await;
        }
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        local_prefix: &str,
    ) -> Result<(), DiscoveryInitError> {
        log::info!("Discovery protocol version: {}", PROTOCOL_VERSION!());

        ServiceBinder::new(&get_offers_addr(public_prefix), &(), self.clone()).bind_with_processor(
            move |_, myself, caller: String, msg: RetrieveOffers| {
                let myself = myself;
                myself.on_get_remote_offers(caller, msg)
            },
        );
        // Subscribe to offer broadcasts.
        {
            let mut prefix_guard = self.inner.lazy_binder_prefix.lock().await;
            if let Some(old_prefix) = (*prefix_guard).replace(local_prefix.to_string()) {
                log::info!("Dropping previous lazy_binder_prefix, and replacing it with new one. old={}, new={}", old_prefix, local_prefix);
            };
        }

        // Only bind broadcasts when re-broadcasts are enabled
        if self.re_broadcast_enabled() {
            self.bind_gsb_broadcast().await.map_or_else(
                |e| {
                    log::warn!("Failed to subscribe to broadcasts. Error: {e}.");
                },
                |_| (),
            );
        }

        Ok(())
    }

    pub async fn bind_gsb_broadcast(&self) -> Result<(), DiscoveryInitError> {
        log::trace!("GsbBroadcastBind");
        let myself = self.clone();

        // /local/market/market-protocol-mk1-offer
        let mut prefix_guard = self.inner.lazy_binder_prefix.lock().await;
        let local_prefix = match (*prefix_guard).take() {
            None => return Ok(()),
            Some(prefix) => prefix,
        };

        let bcast_address = format!("{}/{}", local_prefix.as_str(), OffersBcast::TOPIC);
        ya_net::bind_broadcast_with_caller(
            &bcast_address,
            move |caller, msg: SendBroadcastMessage<OffersBcast>| {
                let myself = myself.clone();
                myself.on_bcast_offers(caller, msg.body().to_owned())
            },
        )
        .await
        .map_err(|e| DiscoveryInitError::from_pair(bcast_address, e))?;

        let myself = self.clone();
        // /local/market/market-protocol-mk1-offer-unsubscribe
        let bcast_address = format!("{}/{}", local_prefix, UnsubscribedOffersBcast::TOPIC);
        ya_net::bind_broadcast_with_caller(
            &bcast_address,
            move |caller, msg: SendBroadcastMessage<UnsubscribedOffersBcast>| {
                let myself = myself.clone();
                myself.on_bcast_unsubscribes(caller, msg.body().to_owned())
            },
        )
        .await
        .map_err(|e| DiscoveryInitError::from_pair(bcast_address, e))?;
        Ok(())
    }

    async fn bcast_receiver_loop(self, mut offers_channel: mpsc::Receiver<(NodeId, OffersBcast)>) {
        while let Some((caller, msg)) = offers_channel.recv().await {
            if !self.inner.ban_cache.is_banned_node(&caller) {
                self.bcast_receiver_loop_step(caller, msg).await.ok();
            } else {
                log::trace!("banned node: {caller}");
            }
        }

        log::debug!("Broadcast receiver loop stopped.");
    }

    async fn bcast_receiver_loop_step(&self, caller: NodeId, msg: OffersBcast) -> Result<(), ()> {
        let start = Instant::now();
        let num_ids_received = msg.offer_ids.len();

        // We should do filtering and getting Offers in single transaction. Otherwise multiple
        // broadcasts can overlap and we will ask other nodes for the same Offers more than once.
        // Note that it wouldn't cause incorrect behavior, because we will add Offers only once.
        // Other attempts to add them will end with error and we will filter all Offers, that already
        // occurred and re-broadcast only new ones.
        // But still it is worth to limit network traffic.
        let filter_out_known_ids = self.inner.offer_handlers.filter_out_known_ids.clone();
        let receive_remote_offers = self.inner.offer_handlers.receive_remote_offers.clone();

        let unknown_offer_ids = filter_out_known_ids.call(caller.to_string(), msg).await?;

        let new_offer_ids = if !unknown_offer_ids.is_empty() {
            let start_remote = Instant::now();
            let offers = self
                .get_remote_offers(caller.to_string(), unknown_offer_ids, 3)
                .await
                .map_err(|e| {
                    self.inner.ban_cache.ban_node(caller);
                    log::debug!("Can't get Offers from [{caller}]. Error: {e}")
                })?;

            let end_remote = Instant::now();
            timing!(
                "market.offers.incoming.get_remote.time",
                start_remote,
                end_remote
            );

            // We still could fail to add some Offers to database. If we fail to add them, we don't
            // want to propagate subscription further.
            receive_remote_offers
                .call(caller.to_string(), OffersRetrieved { offers })
                .await?
        } else {
            vec![]
        };

        if self.re_broadcast_enabled() && !new_offer_ids.is_empty() {
            log::trace!(
                "Propagating {}/{num_ids_received} Offers received from [{caller}].",
                new_offer_ids.len(),
            );

            self.bcast_offers(new_offer_ids)
                .await
                .map_err(|e| log::warn!("Failed to broadcast. Error: {e}"))?;
        }

        let end = Instant::now();
        timing!("market.offers.incoming.time", start, end);
        Ok(())
    }

    async fn on_bcast_offers(self, caller: String, msg: OffersBcast) -> Result<(), ()> {
        let num_ids_received = msg.offer_ids.len();
        log::trace!("Received {num_ids_received} Offers from [{caller}].");

        if msg.offer_ids.is_empty() {
            return Ok(());
        }

        let caller: NodeId = caller.parse().map_err(|_| ())?;
        // We don't want to get overwhelmed by incoming broadcasts, that's why we drop them,
        // if the queue is full.
        match self.inner.offers_receiving_queue.try_send((caller, msg)) {
            Ok(_) => Ok(()),
            Err(_) => {
                log::trace!("Already handling to many broadcasts, skipping...");
                counter!("market.offers.broadcasts.skip", 1);
                Ok(())
            }
        }
    }

    async fn on_get_remote_offers(
        self,
        caller: String,
        msg: RetrieveOffers,
    ) -> Result<Vec<ModelOffer>, DiscoveryRemoteError> {
        log::trace!("[{caller}] asks for {} Offers.", msg.offer_ids.len());
        let get_local_offers = self.inner.offer_handlers.get_local_offers_handler.clone();
        get_local_offers.call(caller, msg).await
    }

    async fn on_bcast_unsubscribes(
        self,
        caller: String,
        msg: UnsubscribedOffersBcast,
    ) -> Result<(), ()> {
        let start = Instant::now();
        let num_received_ids = msg.offer_ids.len();

        log::trace!("Received {num_received_ids} unsubscribed Offers from [{caller}].");
        if msg.offer_ids.is_empty() {
            return Ok(());
        }

        let offer_unsubscribe_handler = self.inner.offer_handlers.offer_unsubscribe_handler.clone();
        let unsubscribed_offer_ids = offer_unsubscribe_handler.call(caller.clone(), msg).await?;

        if self.re_broadcast_enabled() && !unsubscribed_offer_ids.is_empty() {
            log::trace!(
                "Propagating {}/{num_received_ids} unsubscribed Offers received from [{caller}].",
                unsubscribed_offer_ids.len(),
            );

            // No need to retry broadcasting, since we send cyclic broadcasts.
            if let Err(error) = self.bcast_unsubscribes(unsubscribed_offer_ids).await {
                log::error!("Error propagating unsubscribed Offers further: {error}");
            }
        }
        let end = Instant::now();
        timing!("market.offers.unsubscribes.incoming.time", start, end);
        Ok(())
    }

    async fn default_identity(&self) -> Result<NodeId, IdentityError> {
        self.inner.identity.default_identity().await
    }
}

async fn broadcast_offers(node_id: NodeId, offer_ids: Vec<SubscriptionId>) {
    if let Err(e) = net::broadcast(node_id, OffersBcast { offer_ids }).await {
        log::error!("Error broadcasting offers: {e}");
        counter!("market.offers.broadcasts.net_errors", 1);
    };
}

async fn broadcast_unsubscribed(node_id: NodeId, offer_ids: Vec<SubscriptionId>) {
    if let Err(e) = net::broadcast(node_id, UnsubscribedOffersBcast { offer_ids }).await {
        log::error!("Error broadcasting unsubscribed offers: {e}");
        counter!("market.offers.unsubscribes.broadcasts.net_errors", 1);
    };
}
