//! Version handling service bus API.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use ya_client_model::ErrorMessage;
use ya_service_bus::RpcMessage;

pub const BUS_ID: &'static str = "/local/version";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Get {
    pub check: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiscInfo {
    pub test: String,
}

impl Get {
    pub fn show_only() -> Self {
        Get { check: false }
    }

    pub fn with_check() -> Self {
        Get { check: true }
    }
}

impl RpcMessage for Get {
    const ID: &'static str = "check";
    type Item = MiscInfo;
    type Error = ErrorMessage;
}
