use awc::Client;
use futures::prelude::*;
use futures03::compat::Future01CompatExt;
use ya_core_model::net::{GetMessages, Message, SendMessage, SendMessageError};
use ya_service_bus::typed as bus;

pub const HUB_URL: &str = "localhost:8080";

pub const SERVICE_ID: &str = "/local/net";

pub fn init_service() {
    let _ = bus::bind(SERVICE_ID, |command: SendMessage| {
        Client::default()
            .post(HUB_URL)
            .send_json(&command)
            .map_err(|e| SendMessageError(e.to_string()))
            .and_then(|_| Ok(()))
            .compat()
    });
    let _ = bus::bind(SERVICE_ID, |command: GetMessages| {
        Client::default()
            .get(format!("{}/{}", HUB_URL, command.0))
            .send()
            .map_err(|e| SendMessageError(e.to_string()))
            .and_then(|mut x| x.json().map_err(|e| SendMessageError(e.to_string())))
            .and_then(|x: Vec<Message>| Ok(x))
            .compat()
    });
    //bus::service(SERVICE_ID).send(SendMessage(...));
    //bus::service(SERVICE_ID).send(GetMessages("0x123".into()));
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
