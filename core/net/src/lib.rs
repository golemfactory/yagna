use futures::prelude::*;
use futures03::compat::Future01CompatExt;
use futures03::future::Future as Future03;
use ya_service_bus::connection;
use ya_service_bus::{untyped as bus, Error};
use std::net::ToSocketAddrs;

pub const SERVICE_ID: &str = "/net";

#[derive(Default)]
struct SubscribeHelper {}

/// Initialize net module.
pub fn init_service_future(
    hub_addr: &str,
    source_node_id: &str,
) -> impl Future03<Output = Result<(), std::io::Error>> {
    let source_node_id_clone = format!("{}/{}", SERVICE_ID, source_node_id);
    connection::tcp(&hub_addr.to_socket_addrs().unwrap().next().unwrap())
        .and_then(move |c| {
            let connection_ref = connection::connect_with_handler(
                c,
                |_request_id: String, caller: String, addr: String, data: Vec<u8>| {
                    let new_addr: String =
                        format!("/{}", addr.split('/').skip(3).collect::<Vec<_>>().join("/"));
                    /* TODO: request_id? */
                    eprintln!(
                        "[Net Mk1] Incoming message from hub. Called by: {}, addr: {}, new_addr: {}.",
                        caller, addr, new_addr
                    );
                    bus::send(&new_addr, &caller, &data)
                },
            );
            connection_ref
                .bind(source_node_id_clone)
                .and_then(|_| {
                    let _ =
                        bus::subscribe(SERVICE_ID, move |caller: &str, addr: &str, msg: &[u8]| {
                            eprintln!(
                                "[Net Mk1] Sending message to hub. Called by: {}, addr: {}.",
                                caller, addr
                            );
                            connection_ref.call(caller.to_string(), addr.to_string(), Vec::from(msg)).compat()
                        });
                    Ok(())
                })
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)))
        })
        .compat()
}

/// Send message to another node through a hub. Returns a future with the result.
pub fn send_message_future(
    source_node_id: &str,
    destination: &str,
    data: Vec<u8>,
) -> impl Future03<Output = Result<Vec<u8>, Error>> {
    eprintln!(
        "[Net Mk1] Sending message from {} to {}.",
        source_node_id, destination
    );
    bus::send(
        &format!("{}/{}", SERVICE_ID, destination),
        &format!("{}/{}", SERVICE_ID, source_node_id),
        &data,
    )
}
