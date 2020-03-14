use anyhow::{anyhow, Context};
use futures::prelude::*;
use std::net::{SocketAddr, ToSocketAddrs};
use std::str::FromStr;

use crate::net_node_id;
use ya_core_model::{identity, net};
use ya_service_bus::{
    connection, typed as bus, untyped as local_bus, ResponseChunk, RpcEndpoint, PRIVATE_PREFIX,
    PUBLIC_PREFIX,
};

pub const ENV_VAR: &str = "CENTRAL_NET_HOST";
pub const DEFAULT_HOST: &str = "34.244.4.185:7464";

pub fn central_net_host() -> String {
    if let Some(addr_str) = std::env::var(ENV_VAR).ok() {
        addr_str
    } else {
        DEFAULT_HOST.into()
    }
}

pub fn central_net_addr() -> Result<SocketAddr, <SocketAddr as FromStr>::Err> {
    central_net_host().parse()
}

/// Initialize net module on a hub.
pub async fn bind_remote(
    hub_addr: &impl ToSocketAddrs,
    source_node_id: &str,
) -> Result<(), std::io::Error> {
    let hub_addr = hub_addr.to_socket_addrs()?.next().expect("hub addr needed");
    let my_net_node_id = net_node_id(source_node_id);
    log::debug!(
        "connecting Mk1 net server at: {} as {}",
        hub_addr,
        my_net_node_id
    );
    let conn = connection::tcp(hub_addr).await?;

    // connect to hub with forwarding handler
    let own_net_node_id = my_net_node_id.clone();
    let central_bus = connection::connect_with_handler(
        conn,
        move |request_id: String, caller: String, addr: String, data: Vec<u8>| {
            // replaces  /net/<my_id>/test/1 --> ?/test/1
            assert!(addr.starts_with(&own_net_node_id));
            let local_addr: String = addr.replacen(&own_net_node_id, PUBLIC_PREFIX, 1);
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

    // bind /private/net on my local bus and forward all calls to remote bus under /net
    local_bus::subscribe(
        &format!("{}{}", PRIVATE_PREFIX, net::SERVICE_ID),
        move |_caller: &str, addr: &str, msg: &[u8]| {
            // remove /private prefix and post to the hub
            let addr = addr.replacen(PRIVATE_PREFIX, "", 1);
            log::debug!(
                "Sending message to hub. Called by: {}, addr: {}.",
                my_net_node_id,
                addr
            );
            // caller here is always depicted as `local`, so we replace it with our subscriber addr
            central_bus.call(my_net_node_id.clone(), addr.to_string(), Vec::from(msg))
        },
    );

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
            .node_id
            .to_string();

        log::info!("using default identity as network id: {:?}", default_id);

        let net_host = central_net_host();
        crate::bind_remote(&net_host, &default_id)
            .await
            .context(format!("Error binding network service at {}", net_host))
    }
}
