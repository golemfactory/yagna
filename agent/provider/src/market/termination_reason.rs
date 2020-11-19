use derive_more::Display;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use strum::EnumMessage;
use strum_macros::*;

#[derive(Display, EnumMessage, Debug, Clone, PartialEq)]
pub enum BreakReason {
    #[display(fmt = "Failed to initialize. Error: {}", error)]
    #[strum(message = "InitializationError")]
    InitializationError { error: String },
    #[display(fmt = "Agreement expired")]
    #[strum(message = "Expired")]
    Expired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GolemReason {
    #[serde(rename = "message")]
    pub message: String,
    #[serde(rename = "golem.provider.code")]
    pub code: String,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl GolemReason {
    pub fn new(reason: &BreakReason) -> GolemReason {
        GolemReason {
            message: reason.to_string(),
            code: reason.get_message().unwrap_or("Unknown").to_string(),
            extra: HashMap::new(),
        }
    }

    pub fn success() -> GolemReason {
        GolemReason {
            message: "Finished with success.".to_string(),
            code: "Success".to_string(),
            extra: HashMap::new(),
        }
    }
}
