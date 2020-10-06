use actix_rt::Arbiter;
use anyhow::{anyhow, Context};
use futures::prelude::*;
use std::net::{SocketAddr, ToSocketAddrs};
use std::rc::Rc;

use ya_client_model::NodeId;
use ya_core_model::identity::{self, IdentityInfo};
use ya_core_model::net::{self, local as local_net, local::SendBroadcastMessage};
use ya_service_bus::{
    connection, typed as bus, untyped as local_bus, Error, ResponseChunk, RpcEndpoint, RpcMessage,
};

use crate::api::{net_service, parse_from_addr};

pub const CENTRAL_ADDR_ENV_VAR: &str = "CENTRAL_NET_HOST";
pub const DEFAULT_CENTRAL_ADDR: &str = "3.249.139.167:7464";

pub fn central_net_addr() -> std::io::Result<SocketAddr> {
    Ok(std::env::var(CENTRAL_ADDR_ENV_VAR)
        .unwrap_or(DEFAULT_CENTRAL_ADDR.into())
        .to_socket_addrs()?
        .next()
        .expect("central net hub addr needed"))
}

/// Initialize net module on a hub.
pub async fn bind_remote(default_node_id: NodeId, nodes: Vec<NodeId>) -> std::io::Result<()> {
    let hub_addr = central_net_addr()?;
    let conn = connection::tcp(hub_addr).await?;
    let bcast = super::bcast::BCastService::default();
    let bcast_service_id = <SendBroadcastMessage<serde_json::Value> as RpcMessage>::ID;

    // connect to hub with forwarding handler
    let my_net_node_id = net_service(&default_node_id);
    let own_net_nodes: Vec<_> = nodes.iter().map(|id| net_service(id)).collect();

    let forward_call = move |request_id: String, caller: String, addr: String, data: Vec<u8>| {
        let prefix = own_net_nodes
            .iter()
            .find(|&own_net_node_id| addr.starts_with(own_net_node_id));
        if let Some(prefix) = prefix {
            // replaces  /net/<dest_node_id>/test/1 --> /public/test/1
            let local_addr: String = addr.replacen(prefix, net::PUBLIC_PREFIX, 1);
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
        } else {
            return stream::once(future::err(Error::GsbBadRequest(format!(
                "wrong routing: {}; I'll accept only addrs starting with: {:?}",
                addr, own_net_nodes
            ))))
            .left_stream();
        }
    };

    let broadcast_handler = {
        let bcast = bcast.clone();

        move |caller: String, topic: String, msg: Vec<u8>| {
            let endpoints = bcast.resolve(&topic);
            let msg: Rc<[u8]> = msg.into();
            Arbiter::spawn(async move {
                log::trace!("Received broadcast to topic {} from [{}].", &topic, &caller);
                for endpoint in endpoints {
                    let addr = format!("{}/{}", endpoint, bcast_service_id);
                    let _ = local_bus::send(addr.as_ref(), &caller, msg.as_ref()).await;
                }
            })
        }
    };

    let central_bus = connection::connect_with_handler(conn, (forward_call, broadcast_handler));

    // bind my local net service(s) on remote centralised bus under /net/<my_identity>
    for node in &nodes {
        let addr = net_service(node);
        central_bus
            .bind(addr.clone())
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)))?;
        log::info!("network service bound at: {} under: {}", hub_addr, addr);
    }

    // bind /net on my local bus and forward all calls to remote bus under /net
    {
        let central_bus = central_bus.clone();
        let default_caller = default_node_id.to_string();
        local_bus::subscribe(net::BUS_ID, move |_caller: &str, addr: &str, msg: &[u8]| {
            log::debug!(
                "Sending message to hub. Called by: {}, addr: {}.",
                my_net_node_id,
                addr
            );
            // `_caller` here is usually "local", so we replace it with our default node id
            central_bus
                .call(default_caller.clone(), addr.to_string(), Vec::from(msg))
                .map_err(|e| Error::RemoteError(addr.to_string(), e.to_string()))
        });
    }

    // bind /from/<caller>/to/<addr> on my local bus and forward all calls to remote bus under /net
    {
        let central_bus = central_bus.clone();

        local_bus::subscribe("/from", move |_caller: &str, addr: &str, msg: &[u8]| {
            let (from_node, to_addr) = match parse_from_addr(addr) {
                Ok(v) => v,
                Err(e) => return future::err(Error::GsbBadRequest(e.to_string())).left_future(),
            };
            log::debug!("{} is calling {}", from_node, to_addr);
            if !nodes.contains(&from_node) {
                return future::err(Error::GsbBadRequest(format!(
                    "caller: {:?} is not on src list: {:?}",
                    from_node, nodes,
                )))
                .left_future();
            }

            central_bus
                .call(from_node.to_string(), to_addr.clone(), Vec::from(msg))
                .map_err(|e| Error::RemoteError(to_addr, e.to_string()))
                .right_future()
        });
    }

    // Subscribe broadcast on remote
    {
        let bcast = bcast.clone();
        let central_bus = central_bus.clone();

        let _ = bus::bind(local_net::BUS_ID, move |subscribe: local_net::Subscribe| {
            let topic = subscribe.topic().to_owned();
            let (is_new, id) = bcast.add(subscribe);
            let central_bus = central_bus.clone();
            async move {
                log::debug!("Subscribe topic {} on central bus.", topic);
                if is_new {
                    if let Err(e) = central_bus.subscribe(topic.clone()).await {
                        log::error!("fail to subscribe to: {}, {}", topic, e);
                    }
                    log::debug!("Created new topic: {}", topic);
                }
                Ok(id)
            }
        });
    }

    // Send broadcast to remote
    {
        let central_bus = central_bus.clone();
        let addr = format!("{}/{}", local_net::BUS_ID, bcast_service_id);
        let resp: Rc<[u8]> = serde_json::to_vec(&Ok::<(), ()>(())).unwrap().into();
        let _ = local_bus::subscribe(&addr, move |caller: &str, _addr: &str, msg: &[u8]| {
            // TODO: remove unwrap here.
            let ent: SendBroadcastMessage<serde_json::Value> = serde_json::from_slice(msg).unwrap();

            log::trace!(
                "Broadcast msg related to topic {} from [{}].",
                ent.topic(),
                &caller
            );

            let fut = central_bus.broadcast(caller.to_owned(), ent.topic().to_owned(), msg.into());
            let resp = resp.clone();
            async move {
                if let Err(e) = fut.await {
                    Err(Error::GsbFailure(format!("broadcast send failure {}", e)))
                } else {
                    Ok(Vec::from(resp.as_ref()))
                }
            }
        });
    }

    Ok(())
}

pub struct Net;

impl Net {
    pub async fn gsb<Context>(_: Context) -> anyhow::Result<()> {
        let ids: Vec<IdentityInfo> = bus::service(identity::BUS_ID)
            .send(identity::List::default())
            .await
            .map_err(anyhow::Error::msg)??;

        let default_id = ids
            .iter()
            .find(|i| i.is_default)
            .map(|i| i.node_id)
            .ok_or_else(|| anyhow!("no default identity"))?;
        log::info!("using default identity as network id: {:?}", default_id);
        let ids = ids.into_iter().map(|id| id.node_id).collect();

        bind_remote(default_id, ids)
            .await
            .context(format!("Error binding network service"))
    }
}
