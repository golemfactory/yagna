use anyhow::{anyhow, Context, Result};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use ya_client::model::NodeId;
use ya_market_decentralized::MarketService;
use ya_net::bcast;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use super::mock_net::MockNet;

/// Instantiates market test nodes inside one process.
pub struct MarketsNetwork {
    markets: Vec<MarketNode>,
    test_dir: PathBuf,
}

/// Store all object associated with single market
/// for example: Database
#[allow(unused)]
pub struct MarketNode {
    market: Arc<MarketService>,
    name: String,
    /// For now only mock default Identity.
    identity: Identity,
}

impl MarketsNetwork {
    pub async fn new<Str: AsRef<str>>(dir_name: Str) -> Self {
        let test_dir = prepare_test_dir(dir_name).unwrap();

        let bcast = bcast::BCastService::default();
        MockNet::gsb(bcast).await.unwrap();

        MarketsNetwork {
            markets: vec![],
            test_dir,
        }
    }

    pub async fn add_market_instance<Str: AsRef<str>>(
        mut self,
        name: Str,
    ) -> Result<Self, anyhow::Error> {
        let db = self.init_database(name.as_ref())?;
        let market = Arc::new(MarketService::new(&db)?);

        let public_gsb_prefix = format!("/{}", name.as_ref());
        let local_gsb_prefix = format!("/{}", name.as_ref());
        market
            .bind_gsb(&public_gsb_prefix, &local_gsb_prefix)
            .await?;

        let market_node = MarketNode {
            name: name.as_ref().to_string(),
            identity: generate_identity(name.as_ref()),
            market,
        };

        self.markets.push(market_node);
        Ok(self)
    }

    pub fn get_market(&self, name: &str) -> Arc<MarketService> {
        self.markets
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.market.clone())
            .unwrap()
    }

    #[allow(unused)]
    pub fn get_default_id(&self, name: &str) -> Identity {
        self.markets
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.identity.clone())
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
