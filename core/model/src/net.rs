use serde::{Deserialize, Serialize};
use ya_service_bus::{BusMessage, RpcMessage};

pub type NodeID = String; /* TODO: proper NodeID */

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MessageAddress {
    BroadcastAddress { distance: u32 },
    Node(NodeID),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MessageType {
    Request,
    Reply,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub destination: MessageAddress,
    pub module: String,
    pub method: String,
    pub reply_to: NodeID,
    pub request_id: u64,
    pub message_type: MessageType,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SendMessage {
    message: Message,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SendMessageError(String);

impl RpcMessage for SendMessage {
    const ID: &'static str = "send-message";
    type Item = SendMessage; /* TODO should we use the same type for replies? */
    type Error = SendMessageError;
}

#[derive(Serialize, Deserialize, Clone)]
pub enum NetworkStatus {
    ConnectedToCentralizedServer,
    ConnectedToP2PNetwork,
    NotConnected,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GetNetworkStatus {}

impl RpcMessage for GetNetworkStatus {
    const ID: &'static str = "get-network-status";
    type Item = NetworkStatus;
    type Error = ();
}
