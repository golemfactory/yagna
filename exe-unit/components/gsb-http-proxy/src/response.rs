use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GsbHttpCallResponseStreamChunk {
    Header(GsbHttpCallResponseHeader),
    Body(GsbHttpCallResponseBody),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GsbHttpCallResponse {
    pub header: GsbHttpCallResponseHeader,
    pub body: GsbHttpCallResponseBody,
    //
    // pub msg_bytes: Vec<u8>,
    // pub response_headers: HashMap<String, Vec<String>>,
    // pub status_code: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GsbHttpCallResponseHeader {
    pub response_headers: HashMap<String, Vec<String>>,
    pub status_code: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GsbHttpCallResponseBody {
    pub msg_bytes: Vec<u8>,
}

impl GsbHttpCallResponse {
    pub fn with_status_code(code: u16) -> Self {
        GsbHttpCallResponse {
            header: GsbHttpCallResponseHeader {
                status_code: code,
                ..Self::default().header
            },
            ..Self::default()
        }
    }

    pub fn with_message(msg: Vec<u8>, code: u16) -> Self {
        GsbHttpCallResponse {
            header: GsbHttpCallResponseHeader {
                status_code: code,
                ..Self::default().header
            },
            body: GsbHttpCallResponseBody { msg_bytes: msg },
        }
    }

    pub fn new(
        msg_bytes: Vec<u8>,
        response_headers: HashMap<String, Vec<String>>,
        status_code: u16,
    ) -> Self {
        GsbHttpCallResponse {
            header: GsbHttpCallResponseHeader {
                status_code,
                response_headers,
            },
            body: GsbHttpCallResponseBody { msg_bytes },
        }
    }
}

impl Default for GsbHttpCallResponse {
    fn default() -> Self {
        GsbHttpCallResponse {
            header: GsbHttpCallResponseHeader {
                response_headers: HashMap::new(),
                status_code: 0,
            },
            body: GsbHttpCallResponseBody { msg_bytes: vec![] },
        }
    }
}
