use crate::ethaddr::NodeId;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ya_service_bus::RpcMessage;

pub const SERVICE_ID: &str = "/appkey";
pub const BUS_ID: &'static str = "/private/appkey";

pub const DEFAULT_IDENTITY: &str = "primary";
pub const DEFAULT_ROLE: &str = "manager";

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
    pub key: Option<String>,
}

impl Get {
    pub fn with_key(key: String) -> Self {
        Get { key: Some(key) }
    }
}

impl Default for Get {
    fn default() -> Self {
        Get { key: None }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List {
    pub identity: Option<String>,
    pub page: u32,
    pub per_page: u32,
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
