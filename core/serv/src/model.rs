//! Private service control api.

use serde::{Deserialize, Serialize};
use ya_service_bus::RpcMessage;

pub const BUS_ID :&str = "/local/control";

#[derive(Serialize, Deserialize, Default)]
pub struct ShutdownRequest {
    pub graceful : bool
}

impl RpcMessage for ShutdownRequest {
    const ID: &'static str = "ShutdownRequest";
    type Item = ();
    type Error = String;
}
