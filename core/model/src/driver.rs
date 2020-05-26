use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use ya_service_bus::RpcMessage;

pub const BUS_ID: &'static str = "/public/driver";

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ack {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetAccountBalance(String);

impl From<String> for GetAccountBalance {
    fn from(address: String) -> Self {
        GetAccountBalance(address)
    }
}

impl AsRef<String> for GetAccountBalance {
    fn as_ref(&self) -> &String {
        &self.0
    }
}

impl GetAccountBalance {
    pub fn address(&self) -> String {
        self.0.clone()
    }
}

impl RpcMessage for GetAccountBalance {
    const ID: &'static str = "GetAccountBalance";
    type Item = AccountBalanceResult;
    type Error = GenericError;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountBalanceResult {
    pub amount: BigDecimal,
}
