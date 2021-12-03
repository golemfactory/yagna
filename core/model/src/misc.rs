//! Version handling service bus API.

use serde::{Deserialize, Serialize};

use ya_client_model::ErrorMessage;
use ya_service_bus::RpcMessage;
use chrono::DateTime;
use serde_json::Value;

pub const BUS_ID: &'static str = "/local/misc";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiscGet {
    pub check: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthInfo {
    pub is_net_connected: Option<i64>,
    pub last_health_check_worker: Option<i64>,
    pub last_connected_time: Option<i64>,
    pub last_disconnnected_time: Option<i64>,
    pub metrics: Value,
}

impl MiscGet {
    pub fn show_only() -> Self {
        MiscGet { check: false }
    }

    pub fn with_check() -> Self {
        MiscGet { check: true }
    }
}

impl RpcMessage for MiscGet {
    const ID: &'static str = "check";
    type Item = HealthInfo;
    type Error = ErrorMessage;
}
