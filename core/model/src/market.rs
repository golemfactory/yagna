//! Market service bus API.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use ya_client_model::market::{agreement::State, Role};
pub use ya_client_model::market::{Agreement, AgreementListEntry};
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
    pub role: Role,
}

impl GetAgreement {
    pub fn as_provider(agreement_id: String) -> Self {
        GetAgreement::as_role(agreement_id, Role::Provider)
    }

    pub fn as_requestor(agreement_id: String) -> Self {
        GetAgreement::as_role(agreement_id, Role::Requestor)
    }

    pub fn as_role(agreement_id: String, role: Role) -> Self {
        GetAgreement { agreement_id, role }
    }
}

impl RpcMessage for GetAgreement {
    const ID: &'static str = "GetAgreement";
    type Item = Agreement;
    type Error = RpcMessageError;
}

/// Lists all agreements
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListAgreements {
    pub state: Option<State>,
    pub before_date: Option<DateTime<Utc>>,
    pub after_date: Option<DateTime<Utc>>,
    pub app_session_id: Option<String>,
}

impl RpcMessage for ListAgreements {
    const ID: &'static str = "ListAgreements";
    type Item = Vec<AgreementListEntry>;
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
