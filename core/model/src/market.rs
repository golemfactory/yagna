use serde::{Deserialize, Serialize};
use thiserror::*;
use ya_model::market::Agreement;
use ya_service_bus::RpcMessage;

pub const SERVICE_ID: &str = "/market";
pub const BUS_ID: &str = "/private/market";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAgreement {
    pub agreement_id: String,
}

impl GetAgreement {
    pub fn with_id(agreement_id: String) -> Self {
        GetAgreement { agreement_id }
    }
}

impl RpcMessage for GetAgreement {
    const ID: &'static str = "GetAgreement";
    type Item = Agreement;
    type Error = RpcMessageError;
}

#[derive(Clone, Debug, Error, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcMessageError {
    #[error("{0}")]
    Service(String),
    #[error("market api: {0}")]
    Market(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("resource not found")]
    NotFound,
    #[error("configuration error")]
    Forbidden,
    #[error("timeout")]
    Timeout,
}
