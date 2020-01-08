use serde::{Deserialize, Serialize};
use ya_service_bus::RpcMessage;

pub const BUS_ID: &str = "/local/identity";
pub const DEFAULT_IDENTITY: &str = "primary";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Error {}

/// Lists identities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct List {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInfo {
    pub alias: String,
    pub node_id: String,
    pub is_locked: bool,
}

impl RpcMessage for List {
    const ID: &'static str = "List";
    type Item = Vec<IdentityInfo>;
    type Error = Error;
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGenerated {
    pub alias: Option<String>,
}

impl RpcMessage for CreateGenerated {
    const ID: &'static str = "CreateGenerated";
    type Item = IdentityInfo;
    type Error = Error;
}
