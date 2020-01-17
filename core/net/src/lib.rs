use std::net::ToSocketAddrs;

use ya_service_api::constants::{NET_SERVICE_ID, PUBLIC_SERVICE};
use ya_service_bus::{connection, RpcMessage};
use ya_service_bus::{untyped as local_bus, Error};

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

    // bind /net on my local bus
    local_bus::subscribe(
        NET_SERVICE_ID,
        move |caller: &str, addr: &str, msg: &[u8]| {
            log::info!(
                "Sending message to hub. Called by: {}, addr: {}.",
                caller,
                addr
            );
            central_bus.call(caller.to_string(), addr.to_string(), Vec::from(msg))
        },
    );
    Ok(())
}

/// Send message to another node through a hub.
pub async fn send<T: RpcMessage + Unpin>(
    source_node_id: &str,
    destination_service: &str,
    data: &T,
) -> Result<Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>, Error> {
    log::info!(
        "Sending message from {} to {}.",
        source_node_id,
        destination_service
    );
    // send to local bus under /net/0x<destination> eg. 0x789/test

    let raw = local_bus::send(
        &format!(
            "{}/{}/{}",
            NET_SERVICE_ID,
            destination_service,
            <T as RpcMessage>::ID
        ),
        &format!("{}/{}", NET_SERVICE_ID, source_node_id), // caller
        &rmp_serde::encode::to_vec(data)?,
    )
    .await?;
    rmp_serde::from_read_ref(&raw).map_err(From::from)
}
