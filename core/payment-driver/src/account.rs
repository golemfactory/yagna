use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Currency {
    Eth,
    Gnt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Balance {
    pub amount: BigDecimal,
    pub currency: Currency,
}

impl Balance {
    pub fn new(amount: BigDecimal, currency: Currency) -> Balance {
        Balance {
            amount: amount,
            currency: currency,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
