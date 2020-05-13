use std::sync::Arc;

use ya_market_decentralized::Market;


/// Instantiates market test nodes inside one process.
pub struct MarketsNetwork {
    markets: Vec<MarketNode>,
}

/// Store all object associated with single market
/// for example: Database
pub struct MarketNode {
    market: Arc<Market>,
}


impl MarketsNetwork {
    pub fn new() -> Self {
        MarketsNetwork {
            markets: vec![],
        }
    }

    pub fn add_market_instance(self, name: &str) -> Result<Self, anyhow::Error> {
        Err(anyhow::anyhow!("Not implemented"))
    }
}


