use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GsbHttpCallResponseEvent {
    pub index: usize,
    pub timestamp: String,
    pub msg_bytes: Vec<u8>,
    pub response_headers: HashMap<String, Vec<String>>,
    pub status_code: u16,
}

impl GsbHttpCallResponseEvent {
    pub fn default() -> Self {
        GsbHttpCallResponseEvent {
            index: 0,
            msg_bytes: vec![],
            timestamp: Utc::now().naive_local().to_string(),
            response_headers: HashMap::new(),
            status_code: 0,
        }
    }
    pub fn with_status_code(code: u16) -> Self {
        GsbHttpCallResponseEvent {
            status_code: code,
            ..Self::default()
        }
    }

    pub fn with_message(msg: Vec<u8>, code: u16) -> Self {
        GsbHttpCallResponseEvent {
            status_code: code,
            msg_bytes: msg,
            ..Self::default()
        }
    }

    pub fn new(
        index: usize,
        timestamp: String,
        msg_bytes: Vec<u8>,
        response_headers: HashMap<String, Vec<String>>,
        status_code: u16,
    ) -> Self {
        GsbHttpCallResponseEvent {
            index,
            timestamp,
            msg_bytes,
            response_headers,
            status_code,
        }
    }
}
