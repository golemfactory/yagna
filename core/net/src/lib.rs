use awc::Client;
use std::{future::Future, pin::Pin};
use ya_core_model::net::{GetNetworkStatus, NetworkStatus, SendMessage, SendMessageError};
use ya_service_bus::RpcHandler;

// handler: send message to other node

struct SendMessageHandler {}

impl RpcHandler<SendMessage> for SendMessageHandler {
    type Result = Pin<Box<dyn Future<Output = Result<SendMessage, SendMessageError>>>>;

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
    use ya_core_model::net::{Message, MessageAddress, MessageType};

    #[test]
    fn test_serialization() {
        let m: Message = Message {
            //destination: MessageAddress::Node("0x123".into()),
            destination: MessageAddress::BroadcastAddress { distance: 5 },
            module: "module".into(),
            method: "method".into(),
            reply_to: "0x999".into(),
            request_id: 1000,
            message_type: MessageType::Request,
        };
        eprintln!("{}", serde_json::to_string(&m).unwrap())
    }
}
