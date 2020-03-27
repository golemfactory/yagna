use anyhow::{anyhow, Context};
use futures::prelude::*;
use std::net::{SocketAddr, ToSocketAddrs};

use ya_core_model::{ethaddr::NodeId, identity, net};
use ya_service_bus::{
    connection, typed as bus, untyped as local_bus, Error, ResponseChunk, RpcEndpoint,
};

pub const CENTRAL_ADDR_ENV_VAR: &str = "CENTRAL_NET_HOST";
pub const DEFAULT_CENTRAL_ADDR: &str = "34.244.4.185:7464";

pub fn central_net_addr() -> std::io::Result<SocketAddr> {
    Ok(std::env::var(CENTRAL_ADDR_ENV_VAR)
        .unwrap_or(DEFAULT_CENTRAL_ADDR.into())
        .to_socket_addrs()?
        .next()
        .expect("central net hub addr needed"))
}

/// Initialize net module on a hub.
pub async fn bind_remote(source_node_id: &NodeId) -> std::io::Result<()> {
    let hub_addr = central_net_addr()?;
    log::info!("connecting Central Net (Mk1) hub at: {}", hub_addr);
    let conn = connection::tcp(hub_addr).await?;

    // connect to hub with forwarding handler
    let my_net_node_id = crate::net_node_id(source_node_id);
    let own_net_node_id = my_net_node_id.clone();
    let central_bus = connection::connect_with_handler(
        conn,
        move |request_id: String, caller: String, addr: String, data: Vec<u8>| {
            if !addr.starts_with(&own_net_node_id) {
                return stream::once(future::err(Error::GsbBadRequest(format!(
                    "wrong routing: {}; I'll accept only addrs starting with: {}",
                    addr, own_net_node_id
                ))))
                .left_stream();
            }
            // replaces  /net/<src_node_id>/test/1 --> /public/test/1
            let local_addr: String = addr.replacen(&own_net_node_id, net::PUBLIC_PREFIX, 1);
            log::debug!(
                "Incoming msg from = {}, to = {}, fwd to local addr = {}, request_id: {}",
                caller,
                addr,
                local_addr,
                request_id
            );
            // actual forwarding to my local bus
            stream::once(
                local_bus::send(&local_addr, &caller, &data)
                    .and_then(|r| future::ok(ResponseChunk::Full(r))),
            )
            .right_stream()
        },
    );

    // bind my local net service on remote centralised bus under /net/<my_addr>
    central_bus
        .bind(my_net_node_id.clone())
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)))?;

    log::info!(
        "network service bound at: {} as {}",
        hub_addr,
        my_net_node_id
    );

    // bind /net on my local bus and forward all calls to remote bus under /net
    let source_node_id = source_node_id.to_string();
    local_bus::subscribe(net::BUS_ID, move |_caller: &str, addr: &str, msg: &[u8]| {
        log::debug!(
            "Sending message to hub. Called by: {}, addr: {}.",
            my_net_node_id,
            addr
        );
        // `_caller` here is usually "local", so we replace it with our src node id
        central_bus.call(source_node_id.clone(), addr.to_string(), Vec::from(msg))
    });

    Ok(())
}

pub struct Net;

impl Net {
    pub async fn gsb<Context>(_: Context) -> anyhow::Result<()> {
        let default_id = bus::service(identity::BUS_ID)
            .send(identity::Get::ByDefault)
            .await
            .map_err(anyhow::Error::msg)??
            .ok_or(anyhow!("no default identity"))?
            .node_id;

        log::info!("using default identity as network id: {:?}", default_id);

        crate::bind_remote(&default_id)
            .await
            .context(format!("Error binding network service"))
    }
}
