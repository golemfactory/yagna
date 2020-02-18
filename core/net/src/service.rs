use anyhow::Context;
use futures::prelude::*;
use ya_core_model::identity;
use ya_service_api::constants::CENTRAL_NET_HOST;

use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

use std::net::{SocketAddr, ToSocketAddrs};
use std::str::FromStr;
use ya_service_api::constants::{NET_SERVICE_ID, PRIVATE_SERVICE, PUBLIC_SERVICE};
use ya_service_bus::{connection, untyped as local_bus, ResponseChunk};

#[derive(Default)]
struct SubscribeHelper {}

use super::PUBLIC_PREFIX;

pub const NET_ENV_VAR: &str = "CENTRAL_NET_HOST";
pub const DEFAULT_NET_ADDR: &str = "10.30.10.202:7477";

pub fn resolve_default() -> Result<SocketAddr, <SocketAddr as FromStr>::Err> {
    if let Some(addr_str) = std::env::var(NET_ENV_VAR).ok() {
        addr_str.parse()
    } else {
        DEFAULT_NET_ADDR.parse()
    }
}

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
                // TODO: use replacen
                format!("{}{}",
                        PUBLIC_PREFIX,
                        addr.split('/').skip(3).collect::<Vec<_>>().join("/"));
            log::debug!(
                "Incoming message via hub from = {}, to = {}, fwd to local addr = {}",
                caller,
                addr,
                local_addr
            );
            log::debug!("Incoming request_id: {}", request_id);
            // actual forwarding to my local bus
            stream::once(
                local_bus::send(&local_addr, &caller, &data)
                    .and_then(|r| future::ok(ResponseChunk::Full(r))),
            )
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

    {
        let source_node_id = source_node_id.clone();
        let central_bus = central_bus.clone();
        // bind /private/net on my local bus and forward all calls to remote bus under /net
        local_bus::subscribe(
            &format!("{}{}", &*PRIVATE_SERVICE, NET_SERVICE_ID),
            move |_caller: &str, addr: &str, msg: &[u8]| {
                // remove /private prefix and post to the hub
                let addr = addr.replacen(&*PRIVATE_SERVICE, "", 1);
                log::debug!(
                    "Sending message to hub. Called by: {}, addr: {}.",
                    source_node_id,
                    addr
                );
                // caller here is always depicted as `local`, so we replace it with our subscriber addr
                central_bus.call(source_node_id.clone(), addr.to_string(), Vec::from(msg))
            },
        );
    }

    local_bus::subscribe("/net", move |_caller: &str, addr: &str, msg: &[u8]| {
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
            .ok_or(anyhow::Error::msg("no default identity"))?
            .node_id
            .to_string();
        log::info!("using default identity as network id: {:?}", default_id);
        crate::bind_remote(&*CENTRAL_NET_HOST, &default_id)
            .await
            .context(format!(
                "Error binding network service at {} for {}",
                *CENTRAL_NET_HOST, &default_id
            ))?;

        Ok(())
    }
}
