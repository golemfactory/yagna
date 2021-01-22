//! Version handling service bus API.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use ya_client_model::ErrorMessage;
use ya_service_bus::RpcMessage;

pub const BUS_ID: &'static str = "/local/version";

/// Skip upgrading to the latest Yagna release.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skip();

impl RpcMessage for Skip {
    const ID: &'static str = "skip";
    type Item = Option<Release>;
    type Error = ErrorMessage;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Check();

impl RpcMessage for Check {
    const ID: &'static str = "check";
    type Item = Option<Release>;
    type Error = ErrorMessage;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub name: String,
    pub seen: bool,
    pub release_ts: NaiveDateTime,
    pub insertion_ts: Option<NaiveDateTime>,
    pub update_ts: Option<NaiveDateTime>,
}
