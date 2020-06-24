use anyhow::{anyhow, Context, Result};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use ya_client::model::NodeId;
use ya_market_decentralized::protocol::{
    CallbackHandler, Discovery, OfferReceived, OfferUnsubscribed, RetrieveOffers,
};
use ya_market_decentralized::MarketService;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use super::bcast;
use super::mock_net::MockNet;

/// Instantiates market test nodes inside one process.
pub struct MarketsNetwork {
    markets: Vec<MarketNode>,
    discoveries: Vec<DiscoveryNode>,
    test_dir: PathBuf,
    test_name: String,
}

/// Store all object associated with single market
/// for example: Database
#[derive(Clone)]
pub struct MarketNode {
    pub market: Arc<MarketService>,
    pub name: String,
    /// For now only mock default Identity.
    pub identity: Identity,
    /// Direct access to underlying database.
    pub db: DbExecutor,
}

/// Stores mock discovery node, that doesn't include full
/// Market implementation, but only Discovery interface.
/// Necessary to emulate wrong nodes behavior.
pub struct DiscoveryNode {
    discovery: Discovery,
    name: String,
    /// For now only mock default Identity.
    identity: Identity,
}

impl MarketsNetwork {
    /// Remember that dir_name should be unique between all tests.
    /// It will be used to create directories and GSB binding points,
    /// to avoid potential name clashes.
    pub async fn new<Str: AsRef<str>>(dir_name: Str) -> Self {
        let test_dir = prepare_test_dir(&dir_name).unwrap();

        let bcast = bcast::BCastService::default();
        MockNet::gsb(bcast).await.unwrap();

        MarketsNetwork {
            markets: vec![],
            discoveries: vec![],
            test_dir,
            test_name: dir_name.as_ref().to_string(),
        }
    }

    pub async fn add_market_instance<Str: AsRef<str>>(
        mut self,
        name: Str,
    ) -> Result<Self, anyhow::Error> {
        let db = self.init_database(name.as_ref())?;
        let market = Arc::new(MarketService::new(&db)?);

        let public_gsb_prefix = format!("/{}/{}", &self.test_name, name.as_ref());
        let local_gsb_prefix = format!("/{}/{}", &self.test_name, name.as_ref());
        market
            .bind_gsb(&public_gsb_prefix, &local_gsb_prefix)
            .await?;

        let market_node = MarketNode {
            name: name.as_ref().to_string(),
            identity: generate_identity(name.as_ref()),
            market,
            db,
        };

        self.markets.push(market_node);
        Ok(self)
    }

    pub async fn add_discovery_instance<Str: AsRef<str>>(
        mut self,
        name: Str,
        offer_received: impl CallbackHandler<OfferReceived>,
        offer_unsubscribed: impl CallbackHandler<OfferUnsubscribed>,
        retrieve_offers: impl CallbackHandler<RetrieveOffers>,
    ) -> Result<Self, anyhow::Error> {
        let public_gsb_prefix = format!("/{}/{}", &self.test_name, name.as_ref());
        let local_gsb_prefix = format!("/{}/{}", &self.test_name, name.as_ref());

        let discovery = Discovery::new(offer_received, offer_unsubscribed, retrieve_offers)?;
        discovery
            .bind_gsb(&public_gsb_prefix, &local_gsb_prefix)
            .await?;

        let discovery_node = DiscoveryNode {
            name: name.as_ref().to_string(),
            identity: generate_identity(name.as_ref()),
            discovery,
        };

        self.discoveries.push(discovery_node);
        Ok(self)
    }

    pub fn get_market(&self, name: &str) -> Arc<MarketService> {
        self.markets
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.market.clone())
            .unwrap()
    }

    pub fn get_discovery(&self, name: &str) -> Discovery {
        self.discoveries
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.discovery.clone())
            .unwrap()
    }

    pub fn get_node(&self, name: &str) -> MarketNode {
        self.markets
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.clone())
            .unwrap()
    }

    pub fn get_default_id(&self, name: &str) -> Identity {
        // TODO: Could we do this without nesting??
        self.markets
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.identity.clone())
            .unwrap_or_else(|| {
                self.discoveries
                    .iter()
                    .find(|node| node.name == name)
                    .map(|node| node.identity.clone())
                    .unwrap()
            })
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
            .map_err(|error| anyhow!("Failed to create db [{:?}]. Error: {}", db_path, error))?;
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

fn prepare_test_dir<Str: AsRef<str>>(dir_name: Str) -> Result<PathBuf, anyhow::Error> {
    let test_dir: PathBuf = test_data_dir().join(dir_name.as_ref());

    if test_dir.exists() {
        fs::remove_dir_all(&test_dir)
            .with_context(|| format!("Removing test directory: {}", test_dir.display()))?;
    }
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("Creating test directory: {}", test_dir.display()))?;
    Ok(test_dir)
}

fn generate_identity(name: &str) -> Identity {
    let random_node_id: String = thread_rng().sample_iter(&Alphanumeric).take(20).collect();

    Identity {
        name: name.to_string(),
        role: "manager".to_string(),
        identity: NodeId::from(random_node_id[..].as_bytes()),
    }
}

pub mod default {
    use ya_market_decentralized::protocol::{
        DiscoveryRemoteError, OfferReceived, OfferUnsubscribed, Propagate, RetrieveOffers,
        StopPropagateReason,
    };
    use ya_market_decentralized::testing::Offer;

    pub async fn empty_on_offer_received(_msg: OfferReceived) -> Result<Propagate, ()> {
        Ok(Propagate::False(StopPropagateReason::AlreadyExists))
    }

    pub async fn empty_on_offer_unsubscribed(_msg: OfferUnsubscribed) -> Result<Propagate, ()> {
        Ok(Propagate::False(StopPropagateReason::AlreadyUnsubscribed))
    }

    pub async fn empty_on_retrieve_offers(
        _msg: RetrieveOffers,
    ) -> Result<Vec<Offer>, DiscoveryRemoteError> {
        Ok(vec![])
    }
}
