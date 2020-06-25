use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use ya_market_decentralized::protocol::{
    CallbackHandler, Discovery, DiscoveryBuilder, OfferReceived, OfferUnsubscribed, RetrieveOffers,
};
use ya_market_decentralized::testing::mock_offer::generate_identity;
use ya_market_decentralized::testing::{DemandError, OfferError};
use ya_market_decentralized::{Demand, MarketService, Offer, SubscriptionId};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use super::{bcast::BCast, mock_net::MockNet};

#[cfg(feature = "bcast-singleton")]
use super::bcast::singleton::BCastService;
#[cfg(not(feature = "bcast-singleton"))]
use super::bcast::BCastService;

/// Instantiates market test nodes inside one process.
pub struct MarketsNetwork {
    markets: Vec<MarketNode>,
    discoveries: Vec<DiscoveryNode>,
    test_dir: PathBuf,
    test_name: String,
}

/// Store all object associated with single market
/// for example: Database
pub struct MarketNode {
    market: MarketService,
    name: String,
    /// For now only mock default Identity.
    id: Identity,
    /// Direct access to underlying database.
    db: DbExecutor,
}

/// Stores mock discovery node, that doesn't include full
/// Market implementation, but only Discovery interface.
/// Necessary to emulate wrong nodes behavior.
pub struct DiscoveryNode {
    discovery: Discovery,
    name: String,
    /// For now only mock default Identity.
    id: Identity,
}

impl MarketsNetwork {
    /// Remember that dir_name should be unique between all tests.
    /// It will be used to create directories and GSB binding points,
    /// to avoid potential name clashes.
    pub async fn new<Str: AsRef<str>>(test_name: Str) -> Self {
        let test_dir = prepare_test_dir(&test_name).unwrap();

        MockNet::default().bind_gsb();

        MarketsNetwork {
            markets: vec![],
            discoveries: vec![],
            test_dir,
            test_name: test_name.as_ref().to_string(),
        }
    }

    pub async fn add_market_instance<Str: AsRef<str>>(mut self, name: Str) -> Result<Self> {
        let db = self.init_database(name.as_ref())?;
        let market = MarketService::new(&db)?;

        let public_gsb_prefix = format!("/{}/{}", &self.test_name, name.as_ref());
        let local_gsb_prefix = format!("/{}/{}", &self.test_name, name.as_ref());
        market
            .bind_gsb(&public_gsb_prefix, &local_gsb_prefix)
            .await?;

        let market_node = MarketNode {
            name: name.as_ref().to_string(),
            id: generate_identity(name.as_ref()),
            market,
            db,
        };
        BCastService::default().register(&market_node.id.identity, &self.test_name);
        self.markets.push(market_node);
        Ok(self)
    }

    pub async fn add_discovery_instance<Str: AsRef<str>>(
        mut self,
        name: Str,
        offer_received: impl CallbackHandler<OfferReceived>,
        offer_unsubscribed: impl CallbackHandler<OfferUnsubscribed>,
        retrieve_offers: impl CallbackHandler<RetrieveOffers>,
    ) -> Result<Self> {
        let public_gsb_prefix = format!("/{}/{}", &self.test_name, name.as_ref());
        let local_gsb_prefix = format!("/{}/{}", &self.test_name, name.as_ref());

        let discovery = DiscoveryBuilder::default()
            .add_handler(offer_received)
            .add_handler(offer_unsubscribed)
            .add_handler(retrieve_offers)
            .build();
        discovery
            .bind_gsb(&public_gsb_prefix, &local_gsb_prefix)
            .await?;

        let discovery_node = DiscoveryNode {
            name: name.as_ref().to_string(),
            id: generate_identity(name.as_ref()),
            discovery,
        };

        BCastService::default().register(&discovery_node.id.identity, &self.test_name);
        self.discoveries.push(discovery_node);
        Ok(self)
    }

    pub fn get_market(&self, name: &str) -> &MarketService {
        self.markets
            .iter()
            .find(|node| node.name == name)
            .map(|node| &node.market)
            .unwrap()
    }

    pub fn get_market_mut(&mut self, name: &str) -> &mut MarketService {
        self.markets
            .iter_mut()
            .find(|node| node.name == name)
            .map(|node| &mut node.market)
            .unwrap()
    }

    pub fn get_discovery(&self, name: &str) -> Discovery {
        self.discoveries
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.discovery.clone())
            .unwrap()
    }

    pub fn get_default_id(&self, node_name: &str) -> Identity {
        self.markets
            .iter()
            .map(|node| (&node.name, &node.id))
            .chain(self.discoveries.iter().map(|node| (&node.name, &node.id)))
            .find(|&(name, _id)| name == &node_name)
            .map(|(_name, id)| id.clone())
            .unwrap()
    }

    pub fn get_database(&self, name: &str) -> DbExecutor {
        self.markets
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.db.clone())
            .unwrap()
    }

    fn init_database(&self, name: &str) -> Result<DbExecutor> {
        let db_path = self.instance_dir(name);
        let db = DbExecutor::from_data_dir(&db_path, "yagna")
            .map_err(|e| anyhow!("Failed to create db [{:?}]. Error: {}", db_path, e))?;
        Ok(db)
    }

    fn instance_dir(&self, name: &str) -> PathBuf {
        let dir = self.test_dir.join(name);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}

fn test_data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test-workdir")
}

pub fn prepare_test_dir<Str: AsRef<str>>(dir_name: Str) -> Result<PathBuf> {
    let test_dir: PathBuf = test_data_dir().join(dir_name.as_ref());

    if test_dir.exists() {
        fs::remove_dir_all(&test_dir)
            .with_context(|| format!("Removing test directory: {}", test_dir.display()))?;
    }
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("Creating test directory: {}", test_dir.display()))?;
    Ok(test_dir)
}

/// Facilitates waiting for broadcast propagation.
pub async fn wait_for_bcast(
    grace_millis: u64,
    market: &MarketService,
    subscription_id: &SubscriptionId,
    stop_is_ok: bool,
) {
    let steps = 20;
    let wait_step = Duration::from_millis(grace_millis / steps);
    let store = market.matcher.store.clone();
    for _ in 0..steps {
        tokio::time::delay_for(wait_step).await;
        if store.get_offer(&subscription_id).await.is_ok() == stop_is_ok {
            break;
        }
    }
}

#[async_trait::async_trait]
pub trait MarketStore {
    async fn get_offer(&self, id: &SubscriptionId) -> Result<Offer, OfferError>;
    async fn get_demand(&self, id: &SubscriptionId) -> Result<Demand, DemandError>;
}

#[async_trait::async_trait]
impl MarketStore for MarketService {
    async fn get_offer(&self, id: &SubscriptionId) -> Result<Offer, OfferError> {
        self.matcher.store.get_offer(id).await
    }

    async fn get_demand(&self, id: &SubscriptionId) -> Result<Demand, DemandError> {
        self.matcher.store.get_demand(id).await
    }
}

pub mod default {
    use ya_market_decentralized::protocol::{
        DiscoveryRemoteError, OfferReceived, OfferUnsubscribed, Propagate, Reason, RetrieveOffers,
    };
    use ya_market_decentralized::Offer;

    pub async fn empty_on_offer_received(
        _caller: String,
        _msg: OfferReceived,
    ) -> Result<Propagate, ()> {
        Ok(Propagate::No(Reason::AlreadyExists))
    }

    pub async fn empty_on_offer_unsubscribed(
        _caller: String,
        _msg: OfferUnsubscribed,
    ) -> Result<Propagate, ()> {
        Ok(Propagate::No(Reason::Unsubscribed))
    }

    pub async fn empty_on_retrieve_offers(
        _caller: String,
        _msg: RetrieveOffers,
    ) -> Result<Vec<Offer>, DiscoveryRemoteError> {
        Ok(vec![])
    }
}
