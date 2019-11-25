use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin};
use ya_service_bus::{BusMessage, BusPath, RpcHandler, RpcMessage};

// handler: send message to other node

#[derive(Serialize, Deserialize, Clone)]
enum MessageAddress {
    BroadcastAddress { distance: u32 },
    Node(String), /* TODO: replace with NodeID */
}

#[derive(Serialize, Deserialize, Clone)]
enum MessageType {
    Request,
    Reply,
    Error,
}

#[derive(Serialize, Deserialize, Clone)]
struct Message {
    destination: MessageAddress,
    module: String,
    method: String,
    reply_to: String, /* TODO: replace with NodeID */
    request_id: u64,
    message_type: MessageType,
}

#[derive(Serialize, Deserialize, Clone)]
struct SendMessage {
    message: Message,
}

impl BusMessage for SendMessage {}

impl RpcMessage for SendMessage {
    const ID: &'static str = "send-message";
    type Reply = SendMessage; /* TODO should we use the same type for replies? */
}

struct SendMessageHandler {}

impl RpcHandler<SendMessage> for SendMessageHandler {
    type Result = Pin<Box<dyn Future<Output = SendMessage>>>;

    fn handle(&mut self, _caller: BusPath, _msg: SendMessage) -> Self::Result {
        unimplemented!()
    }
}

// handler: network status

#[derive(Serialize, Deserialize, Clone)]
enum NetworkStatus {
    ConnectedToCentralizedServer,
    ConnectedToP2PNetwork,
    NotConnected,
}

impl BusMessage for NetworkStatus {}

#[derive(Serialize, Deserialize, Clone)]
struct GetNetworkStatus {}

impl BusMessage for GetNetworkStatus {}

impl RpcMessage for GetNetworkStatus {
    const ID: &'static str = "get-network-status";
    type Reply = NetworkStatus;
}

struct GetNetworkStatusHandler {}

impl RpcHandler<GetNetworkStatus> for GetNetworkStatusHandler {
    type Result = Pin<Box<dyn Future<Output = NetworkStatus>>>;

    fn handle(&mut self, _caller: BusPath, _msg: GetNetworkStatus) -> Self::Result {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
