use awc::Client;
use futures::prelude::*;
use futures03::compat::Future01CompatExt;
use std::pin::Pin;
use ya_core_model::net::{GetMessages, Message, SendMessage, SendMessageError};
use ya_service_bus::connection;
use ya_service_bus::connection::LocalRouterHandler;
use ya_service_bus::untyped::RawHandler;
use ya_service_bus::{untyped as bus, Error};

pub const HUB_ADDR: &str = "127.0.0.1:8245";

pub const SERVICE_ID: &str = "/net";

#[derive(Default)]
struct SubscribeHelper {}

pub fn init_service() {
    /* TODO: launch this; currently this function does nothing */
    let connection = connection::tcp(&HUB_ADDR.parse().unwrap()).and_then(|c| {
        let c_ref = connection::connect_with_handler(
            c,
            |r_id: String, caller: String, addr: String, data: Vec<u8>| {
                /* TODO: process data before sending to the bus */
                bus::send(&addr, &caller, &data)
            },
        );
        /* TODO: register with the local node id */
        c_ref.bind("0x123");
        let _ = bus::subscribe(SERVICE_ID, move |caller: &str, addr: &str, msg: &[u8]| {
            eprintln!("[Net Mk1] Called by: {}, addr: {}.", caller, addr);
            /* TODO: 1. get address. 2. forward to router through connection.rs */
            /* 2. */
            c_ref.call(caller, addr, msg);
            futures03::future::ok(vec![])
        });
        Ok(())
    });
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
