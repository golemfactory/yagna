use awc::Client;
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin};
use ya_service_bus::{BusMessage, RpcHandler, RpcMessage};

pub type NodeID = String; /* TODO: proper NodeID */

// handler: send message to other node

#[derive(Serialize, Deserialize, Clone)]
enum MessageAddress {
    BroadcastAddress { distance: u32 },
    Node(NodeID),
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
    reply_to: NodeID,
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
    type Item = SendMessage; /* TODO should we use the same type for replies? */
    type Error = (); /* TODO */
}

struct SendMessageHandler {}

impl RpcHandler<SendMessage> for SendMessageHandler {
    type Result = Pin<Box<dyn Future<Output = Result<SendMessage, ()>>>>;

    fn handle(&mut self, _caller: &str, _msg: SendMessage) -> Self::Result {
        unimplemented!()
        /* TODO */
        //futures::future::ready(Ok(NetworkStatus::NotConnected))
        /*Box::pin(
            Client::default()
                .get("http://localhost:8000")
                .send()
                .and_then(|response| futures::future::ready(Ok(SendMessage { message: None }))),
        )*/
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
    type Item = NetworkStatus;
    type Error = ();
}

struct GetNetworkStatusHandler {}

impl RpcHandler<GetNetworkStatus> for GetNetworkStatusHandler {
    type Result = futures::future::Ready<Result<NetworkStatus, ()>>; /* dynamic version: Pin<Box<dyn Future<Output = NetworkStatus>>>*/

    fn handle(&mut self, _caller: &str, _msg: GetNetworkStatus) -> Self::Result {
        /* TODO get real network status */
        futures::future::ready(Ok(NetworkStatus::NotConnected)) /* dynamic version: add Box::pin(...) */
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
