use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErrorMessage {
    #[serde(rename = "message", skip_serializing_if = "Option::is_none", default)]
    pub message: Option<String>,
}

impl ErrorMessage {
    pub fn new() -> ErrorMessage {
        ErrorMessage { message: None }
    }
}
