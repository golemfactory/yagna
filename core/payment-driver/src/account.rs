use ethereum_types::U256;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Chain {
    Mainnet,
    Rinkeby,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Currency {
    Eth,
    Gnt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub currency: Currency,
    pub amount: U256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBalance {
    pub base_currency: Balance,
    pub gas: Option<Balance>,
}
