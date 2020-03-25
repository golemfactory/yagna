//! Market service bus API.
use serde::{Deserialize, Serialize};
pub use ya_model::market::Agreement;
use ya_service_bus::RpcMessage;

/// Public Market bus address.
pub const BUS_ID: &str = "/public/market";

/// Returns the [Agreement](../../ya_model/market/agreement/struct.Agreement.html).
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
