//! Market service bus API.
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::bus::GsbBindPoints;
use ya_client_model::market::{agreement::State, Role};
pub use ya_client_model::market::{Agreement, AgreementListEntry};
use ya_client_model::NodeId;
use ya_service_bus::RpcMessage;

/// Public Market bus address.
pub const BUS_ID: &str = "/public/market";
pub const BUS_SERVICE_NAME: &str = "market";

/// Use None for default bindpoints value.
/// Override in case you are creating tests that need to separate
/// bindpoints for different instances of Market.
pub fn bus_bindpoints(base: Option<GsbBindPoints>) -> GsbBindPoints {
    match base {
        Some(base) => base.service(BUS_SERVICE_NAME),
        None => GsbBindPoints::default().service(BUS_SERVICE_NAME),
    }
}

/// Internal Market bus address.
pub mod local {
    pub const BUS_ID: &str = "/local/market";
    pub const BUS_DISCOVERY: &str = "market-discovery";

    /// Builds the discovery bus endpoint with a custom prefix.
    pub fn build_discovery_endpoint(prefix: &str) -> String {
        format!("{}/{}", prefix, BUS_DISCOVERY)
    }

    /// Builds the discovery bus endpoint with the default prefix.
    pub fn discovery_endpoint() -> String {
        build_discovery_endpoint(BUS_ID)
    }
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

/// Returns the Agreement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLastBcastTs;

impl RpcMessage for GetLastBcastTs {
    const ID: &'static str = "GetLastBcastTs";
    type Item = DateTime<Utc>;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundGolemBase {
    pub wallet: Option<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundGolemBaseResponse {
    pub wallet: NodeId,
    pub balance: BigDecimal,
}

impl RpcMessage for FundGolemBase {
    const ID: &'static str = "FundGolemBase";
    type Item = FundGolemBaseResponse;
    type Error = RpcMessageError;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetGolemBaseBalance {
    pub wallet: Option<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetGolemBaseBalanceResponse {
    pub wallet: NodeId,
    pub balance: BigDecimal,
    pub token: String,
}

impl RpcMessage for GetGolemBaseBalance {
    const ID: &'static str = "GetGolemBaseBalance";
    type Item = GetGolemBaseBalanceResponse;
    type Error = RpcMessageError;
}
