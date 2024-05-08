use crate::error::HttpProxyStatusError;
use crate::response::GsbHttpCallResponseEvent;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use ya_service_bus::{RpcMessage, RpcStreamMessage};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GsbHttpCallMessage {
    pub method: String,
    pub path: String,
    pub body: Option<Vec<u8>>,
    pub headers: HashMap<String, Vec<String>>,
}

impl RpcMessage for GsbHttpCallMessage {
    const ID: &'static str = "GsbHttpCallMessage";
    type Item = GsbHttpCallResponseEvent;
    type Error = HttpProxyStatusError;
}

impl RpcStreamMessage for GsbHttpCallMessage {
    const ID: &'static str = "GsbHttpCallMessage";
    type Item = GsbHttpCallResponseEvent;
    type Error = HttpProxyStatusError;
}
