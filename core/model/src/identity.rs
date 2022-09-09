use serde::{Deserialize, Serialize};
use thiserror::Error;
use ya_client_model::NodeId;
use ya_service_bus::RpcMessage;

pub const BUS_ID: &str = "/local/identity";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ack {}

#[derive(Clone, Debug, Serialize, Deserialize, Error)]
pub enum Error {
    #[error("initialization failed {0}")]
    Init(String),
    #[error("given alias or key already exists")]
    AlreadyExists,
    #[error("node {0:?} not found")]
    NodeNotFound(Box<NodeId>),
    #[error("{0}")]
    InternalErr(String),
    #[error("bad keystore format: {0}")]
    BadKeyStoreFormat(String),
}

impl Error {
    pub fn new_init_err(e: impl std::fmt::Display) -> Self {
        Error::Init(e.to_string())
    }

    pub fn new_err_msg(e: impl std::fmt::Display) -> Self {
        Error::InternalErr(e.to_string())
    }

    pub fn keystore_format(e: impl std::fmt::Display) -> Self {
        Error::BadKeyStoreFormat(e.to_string())
    }
}

/// Lists identities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct List {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInfo {
    #[serde(default)]
    pub alias: Option<String>,
    pub node_id: NodeId,
    pub is_locked: bool,
    pub is_default: bool,
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
    pub from_keystore: Option<String>,
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
    ByNodeId(NodeId),
    ByAlias(String),
    ByDefault,
}

impl RpcMessage for Get {
    const ID: &'static str = "Get";
    type Item = Option<IdentityInfo>;
    type Error = Error;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Update {
    pub node_id: NodeId,
    pub alias: Option<String>,
    pub set_default: bool,
}

impl Update {
    pub fn with_id(node_id: NodeId) -> Self {
        Self {
            node_id,
            alias: Default::default(),
            set_default: Default::default(),
        }
    }

    pub fn with_alias(mut self, alias: impl Into<Option<String>>) -> Self {
        self.alias = alias.into();
        self
    }

    pub fn with_default(mut self, set_default: bool) -> Self {
        self.set_default = set_default;
        self
    }
}

impl RpcMessage for Update {
    const ID: &'static str = "Update";
    type Item = IdentityInfo;
    type Error = Error;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Lock {
    pub node_id: NodeId,
    pub set_password: Option<String>,
}

impl Lock {
    pub fn with_id(node_id: NodeId) -> Self {
        Self {
            node_id,
            set_password: Default::default(),
        }
    }

    pub fn with_set_password(mut self, set_password: impl Into<Option<String>>) -> Self {
        self.set_password = set_password.into();
        self
    }
}

impl RpcMessage for Lock {
    const ID: &'static str = "Lock";
    type Item = IdentityInfo;
    type Error = Error;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Unlock {
    pub node_id: NodeId,
    pub password: String,
}

impl RpcMessage for Unlock {
    const ID: &'static str = "Unlock";
    type Item = IdentityInfo;
    type Error = Error;
}

impl Unlock {
    pub fn with_id(node_id: NodeId, password: String) -> Self {
        Self { node_id, password }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DropId {
    pub node_id: NodeId,
}

impl DropId {
    pub fn with_id(node_id: NodeId) -> Self {
        Self { node_id }
    }
}

impl RpcMessage for DropId {
    const ID: &'static str = "DropId";
    type Item = IdentityInfo;
    type Error = Error;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sign {
    pub node_id: NodeId,
    pub payload: Vec<u8>,
}

impl RpcMessage for Sign {
    const ID: &'static str = "Sign";
    type Item = Vec<u8>;
    type Error = Error;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscribe {
    pub endpoint: String,
}

impl RpcMessage for Subscribe {
    const ID: &'static str = "Subscribe";
    type Item = Ack;
    type Error = Error;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPubKey(pub NodeId);

impl RpcMessage for GetPubKey {
    const ID: &'static str = "GetPubKey";
    type Item = Vec<u8>;
    type Error = Error;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetKeyFile(pub NodeId);

impl RpcMessage for GetKeyFile {
    const ID: &'static str = "GetKeyFile";
    type Item = String;
    type Error = Error;
}

pub mod event {
    use super::Error;
    use serde::{Deserialize, Serialize};
    use ya_client_model::NodeId;
    use ya_service_bus::RpcMessage;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub enum Event {
        AccountLocked { identity: NodeId },
        AccountUnlocked { identity: NodeId },
    }

    impl RpcMessage for Event {
        const ID: &'static str = "Identity__Event";
        type Item = ();
        type Error = Error;
    }
}
