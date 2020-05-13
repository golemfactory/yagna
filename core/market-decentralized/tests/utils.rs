use anyhow::{anyhow, Context};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use ya_market_decentralized::Market;
use ya_persistence::executor::DbExecutor;

/// Instantiates market test nodes inside one process.
pub struct MarketsNetwork {
    markets: Vec<MarketNode>,
    test_dir: PathBuf,
}

/// Store all object associated with single market
/// for example: Database
pub struct MarketNode {
    market: Arc<Market>,
    name: String,
}

impl MarketsNetwork {
    pub fn new<Str: AsRef<str>>(dir_name: Str) -> Self {
        let test_dir = prepare_test_dir(dir_name).unwrap();

        MarketsNetwork {
            markets: vec![],
            test_dir,
        }
    }

    pub async fn add_market_instance<Str: AsRef<str>>(
        mut self,
        name: Str,
    ) -> Result<Self, anyhow::Error> {
        let db = DbExecutor::from_data_dir(&self.test_dir, name.as_ref())?;
        let market = Arc::new(Market::new(&db)?);

        market.bind_gsb(name.as_ref().to_string()).await?;

        let market_node = MarketNode {
            name: name.as_ref().to_string(),
            market,
        };

        self.markets.push(market_node);
        Ok(self)
    }

    pub fn get_market(&self, name: &str) -> Arc<Market> {
        self.markets
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.market.clone())
            .unwrap()
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
