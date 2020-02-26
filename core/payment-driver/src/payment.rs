use chrono::{DateTime, Utc};
use ethereum_types::{Address, U256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentConfirmation {
    pub confirmation: Vec<u8>,
}

impl PaymentConfirmation {
    pub fn from(bytes: &[u8]) -> PaymentConfirmation {
        PaymentConfirmation {
            confirmation: bytes.to_vec(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentStatus {
    Ok(PaymentConfirmation),
    NotYet,
    NotEnoughFunds,
    NotEnoughGas,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentDetails {
    pub recipient: Address,
    pub amount: U256,
    pub date: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentAmount {
    pub base_currency_amount: U256,
    pub gas_amount: Option<U256>,
}
