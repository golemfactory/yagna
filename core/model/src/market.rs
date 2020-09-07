//! Market service bus API.
use serde::{Deserialize, Serialize};
pub use ya_client_model::market::Agreement;
use ya_service_bus::RpcMessage;

/// Public Market bus address.
pub const BUS_ID: &str = "/public/market";

/// Internal Market bus address.
pub mod local {
    pub const BUS_ID: &str = "/local/market";
}

/// Returns the Agreement.
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

/// Error message for market service bus API.
#[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcMessageError {
    #[error("Service error: {0}")]
    Service(String),
    #[error("Market API error: {0}")]
    Market(String),
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Timeout")]
    Timeout,
}
