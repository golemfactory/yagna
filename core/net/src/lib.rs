pub mod service;

use std::net::ToSocketAddrs;

use ya_service_api::constants::{NET_SERVICE_ID, PRIVATE_SERVICE, PUBLIC_SERVICE};
use ya_service_bus::{connection, untyped as local_bus};

#[derive(Default)]
struct SubscribeHelper {}

/// Initialize net module on a hub.
pub async fn bind_remote(
    hub_addr: &impl ToSocketAddrs,
    source_node_id: &str,
) -> Result<(), std::io::Error> {
    let hub_addr = hub_addr.to_socket_addrs()?.next().expect("hub addr needed");
    log::debug!("connecting Mk1 net server at: {}", hub_addr);
    let conn = connection::tcp(hub_addr).await?;

    // connect with hub with forwarding handler
    let central_bus = connection::connect_with_handler(
        conn,
        |request_id: String, caller: String, addr: String, data: Vec<u8>| {
            let local_addr: String =
                // replaces  /net/0x789/test/1 --> /public/test/1
                format!("{}/{}",
                        &*PUBLIC_SERVICE,
                        addr.split('/').skip(3).collect::<Vec<_>>().join("/"));
            log::debug!(
                "Incoming message via hub from = {}, to = {}, fwd to local addr = {}",
                caller,
                addr,
                local_addr
            );
            log::debug!("Incoming request_id: {}", request_id);
            // actual forwarding to my local bus
            local_bus::send(&local_addr, &caller, &data)
        },
    );

    // bind my local net service on remote centralised bus under /net/<my_addr>
    let source_node_id = format!("{}/{}", NET_SERVICE_ID, source_node_id);
    central_bus
        .bind(source_node_id.clone())
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)))?;
    log::info!(
        "network service bound at: {} as {}",
        hub_addr,
        source_node_id
    );

    // bind /private/net on my local bus and forward all calls to remote bus under /net
    local_bus::subscribe(
        &format!("{}{}", &*PRIVATE_SERVICE, NET_SERVICE_ID),
        move |_caller: &str, addr: &str, msg: &[u8]| {
            // remove /private prefix and post to the hub
            let addr = addr.replacen(&*PRIVATE_SERVICE, "", 1);
            log::info!(
                "Sending message to hub. Called by: {}, addr: {}.",
                source_node_id,
                addr
            );
            // caller here is always depicted as `local`, so we replace it with our subscriber addr
            central_bus.call(source_node_id.clone(), addr.to_string(), Vec::from(msg))
        },
    );
    Ok(())
}
