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
