use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ya_client_model::NodeId;
use ya_service_bus::RpcMessage;

pub const BUS_ID: &'static str = "/local/appkey";

pub const DEFAULT_ROLE: &str = "manager";

const DEFAULT_PAGE_SIZE: u32 = 20;

#[derive(Clone, Error, Debug, Serialize, Deserialize)]
#[error("appkey error [{code}]: {message}")]
pub struct Error {
    pub code: u32,
    pub message: String,
}

impl Error {
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self {
            code: 500,
            message: e.to_string(),
        }
    }

    pub fn bad_request(e: impl std::fmt::Display) -> Self {
        Self {
            code: 400,
            message: e.to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Create {
    pub name: String,
    pub role: String,
    pub identity: NodeId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Get {
    pub key: String,
}

impl Get {
    pub fn with_key(key: String) -> Self {
        Get { key }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List {
    pub identity: Option<String>,
    pub page: u32,
    pub per_page: u32,
}

impl List {
    pub fn with_identity(identity: impl ToString) -> Self {
        List {
            identity: Some(identity.to_string()),
            page: 1,
            per_page: DEFAULT_PAGE_SIZE,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Remove {
    pub name: String,
    pub identity: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppKey {
    pub name: String,
    pub key: String,
    pub role: String,
    pub identity: NodeId,
    pub created_date: NaiveDateTime,
}

impl RpcMessage for Create {
    const ID: &'static str = "Create";
    type Item = String;
    type Error = Error;
}

impl RpcMessage for Get {
    const ID: &'static str = "Get";
    type Item = AppKey;
    type Error = Error;
}

impl RpcMessage for List {
    const ID: &'static str = "List";
    type Item = (Vec<AppKey>, u32);
    type Error = Error;
}

impl RpcMessage for Remove {
    const ID: &'static str = "Remove";
    type Item = ();
    type Error = Error;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscribe {
    pub endpoint: String,
}

impl RpcMessage for Subscribe {
    const ID: &'static str = "Subscribe";
    type Item = u64;
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
        NewKey { identity: NodeId },
    }

    impl RpcMessage for Event {
        const ID: &'static str = "AppKey__Event";
        type Item = ();
        type Error = Error;
    }
}
