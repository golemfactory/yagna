use serde::{Deserialize, Serialize};
use ya_model::market::Agreement;
use ya_service_bus::RpcMessage;

pub const SERVICE_ID: &str = "/market";
pub const BUS_ID: &str = "/private/market";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAgreement {
    pub agreement_id: String,
    pub timeout: Option<u32>,
}

impl RpcMessage for GetAgreement {
    const ID: &'static str = "GetAgreement";
    type Item = Agreement;
    type Error = RpcMessageError;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcMessageError {
    Service(String),
    Market(String),
    BadRequest(String),
    NotFound,
    Forbidden,
    Timeout,
}
