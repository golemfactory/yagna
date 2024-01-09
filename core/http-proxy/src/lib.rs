use serde::{Deserialize, Serialize};
use ya_service_bus::RpcMessage;
use ya_service_bus::RpcStreamMessage;

pub const BUS_ID: &str = "/public/http-proxy";

#[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
pub enum HttpProxyStatusError {
    #[error("{0}")]
    RuntimeException(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct GsbHttpCall {
    pub host: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GsbHttpCallEvent {
    pub index: usize,
    pub timestamp: String,
    pub val: String,
}

impl RpcMessage for GsbHttpCall {
    const ID: &'static str = "GsbHttpCall";
    type Item = GsbHttpCallEvent;
    type Error = HttpProxyStatusError;
}

impl RpcStreamMessage for GsbHttpCall {
    const ID: &'static str = "GsbHttpCall";
    type Item = GsbHttpCallEvent;
    type Error = HttpProxyStatusError;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {}
}
