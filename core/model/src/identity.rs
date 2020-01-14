use serde::{Deserialize, Serialize};
use ya_service_bus::RpcMessage;
use crate::ethaddr::NodeId;

pub const BUS_ID: &str = "/local/identity";
pub const DEFAULT_IDENTITY: &str = "primary";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Error {
    Init(String),
    AlreadyExists,
    InternalErr(String),
}

impl Error {
    pub fn new_init_err(e: impl std::fmt::Display) -> Self {
        Error::Init(e.to_string())
    }

    pub fn new_err_msg(e: impl std::fmt::Display) -> Self {
        Error::InternalErr(e.to_string())
    }
}

/// Lists identities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct List {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInfo {
    pub alias: String,
    pub node_id: NodeId,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum Get {
    ByNodeId(String),
    ByAlias(String),
}

impl RpcMessage for Get {
    const ID: &'static str = "Get";
    type Item = Option<IdentityInfo>;
    type Error = Error;
}
