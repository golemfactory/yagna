use crate::account::Currency;
use chrono::{DateTime, Utc};
use ethereum_types::{Address, U256};

#[allow(unused)]
pub struct PaymentConfirmation {
    confirmation: Vec<u8>,
}

impl PaymentConfirmation {
    pub fn from(bytes: &[u8]) -> PaymentConfirmation {
        PaymentConfirmation {
            confirmation: bytes.to_vec(),
        }
    }
}

#[allow(unused)]
pub struct Balance {
    currency: Currency,
    amount: U256,
}

#[allow(unused)]
pub struct AccountBalance {
    base_currency: Balance,
    gas: Option<Balance>,
}

#[allow(unused)]
pub enum TransactionStatus {
    Ok,
    NotYet,
    NotFound,
    NotEnoughFunds,
    NotEnoughGas,
}

#[allow(unused)]
pub struct PaymentStatus {
    status: TransactionStatus,
    confirmation: Option<PaymentConfirmation>,
}

#[allow(unused)]
pub struct PaymentDetails {
    receiver: Address,
    amount: U256,
    date: Option<DateTime<Utc>>,
}

#[allow(unused)]
pub struct PaymentAmount {
    base_currency_amount: U256,
    gas_amount: Option<U256>,
}
