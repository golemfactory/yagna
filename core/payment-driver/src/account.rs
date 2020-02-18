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
    pub amount: U256,
    pub currency: Currency,
}

impl Balance {
    pub fn new(amount: U256, currency: Currency) -> Balance {
        Balance {
            amount: amount,
            currency: currency
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBalance {
    pub base_currency: Balance,
    pub gas: Option<Balance>,
}

impl AccountBalance {
    pub fn new(base_currency: Balance, gas: Option<Balance>) -> AccountBalance {
        AccountBalance {
            base_currency: base_currency,
            gas: gas,
        }
    }
}
