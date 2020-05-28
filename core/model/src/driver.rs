use bigdecimal::BigDecimal;
use bitflags::bitflags;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use ya_service_bus::RpcMessage;

pub const BUS_ID: &'static str = "/public/driver";

// ************************** ERROR **************************

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
#[error("{inner}")]
pub struct GenericError {
    inner: String,
}

impl GenericError {
    pub fn new<T: Display>(e: T) -> Self {
        let inner = e.to_string();
        Self { inner }
    }
}

// ************************** ACK **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ack {}

// ************************** ACCOUNT **************************

bitflags! {
    pub struct AccountMode : usize {
        const NONE = 0b000;
        const RECV = 0b001;
        const SEND = 0b010;
        const ALL = Self::RECV.bits | Self::SEND.bits;
    }
}

// ************************** CURRENCY **************************

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Currency {
    Eth,
    Gnt,
}

// ************************** BALANCE **************************

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Balance {
    pub amount: BigDecimal,
    pub currency: Currency,
}

impl Balance {
    pub fn new(amount: BigDecimal, currency: Currency) -> Balance {
        Balance { amount, currency }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountBalance {
    pub base_currency: Balance,
    pub gas: Option<Balance>,
}

impl AccountBalance {
    pub fn new(base_currency: Balance, gas: Option<Balance>) -> AccountBalance {
        AccountBalance { base_currency, gas }
    }
}

// ************************** PAYMENT **************************

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentStatus {
    Ok(PaymentConfirmation),
    NotYet,
    NotEnoughFunds,
    NotEnoughGas,
    Unknown,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentAmount {
    pub base_currency_amount: BigDecimal,
    pub gas_amount: Option<BigDecimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentDetails {
    pub recipient: String,
    pub sender: String,
    pub amount: BigDecimal,
    pub date: Option<DateTime<Utc>>,
}

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

// ************************** GET ACCOUNT BALANCE **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetAccountBalance(String);

impl From<String> for GetAccountBalance {
    fn from(address: String) -> Self {
        GetAccountBalance(address)
    }
}

impl GetAccountBalance {
    pub fn address(&self) -> String {
        self.0.clone()
    }
}

impl RpcMessage for GetAccountBalance {
    const ID: &'static str = "GetAccountBalance";
    type Item = AccountBalance;
    type Error = GenericError;
}

// ************************** GET PAYMENT STATUS **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetPaymentStatus(String);

impl From<String> for GetPaymentStatus {
    fn from(invoice_id: String) -> Self {
        GetPaymentStatus(invoice_id)
    }
}

impl GetPaymentStatus {
    pub fn invoice_id(&self) -> String {
        self.0.clone()
    }
}

impl RpcMessage for GetPaymentStatus {
    const ID: &'static str = "GetPaymentStatus";
    type Item = PaymentStatus;
    type Error = GenericError;
}
