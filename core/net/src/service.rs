use actix_rt::Arbiter;
use anyhow::{anyhow, Context};
use futures::prelude::*;
use std::net::{SocketAddr, ToSocketAddrs};
use std::rc::Rc;
use ya_client_model::NodeId;
use ya_core_model::identity::IdentityInfo;
use ya_core_model::net::local as local_net;
use ya_core_model::net::local::SendBroadcastMessage;
use ya_core_model::{identity, net};
use ya_service_bus::{
    connection, typed as bus, untyped as local_bus, Error, ResponseChunk, RpcEndpoint, RpcMessage,
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

#[inline]
fn net_node_id(node_id: &NodeId) -> String {
    format!("{}/{:?}", net::BUS_ID, node_id)
}

fn parse_from_addr(from_addr: &str) -> anyhow::Result<(NodeId, NodeId, &str)> {
    let mut it = from_addr.split("/").fuse();
    if let (Some(""), Some("from"), Some(from_node_id), Some("to"), Some(to_node_id)) =
        (it.next(), it.next(), it.next(), it.next(), it.next())
    {
        let prefix = 10 + from_node_id.len() + to_node_id.len();
        let service_id = &from_addr[prefix..];
        if service_id.starts_with('/') {
            return Ok((from_node_id.parse()?, to_node_id.parse()?, service_id));
        }
    }
    Err(anyhow!("invalid net-from destination: {}", from_addr))
}

/// Initialize net module on a hub.
pub async fn bind_remote(default_node_id: NodeId, nodes: Vec<NodeId>) -> std::io::Result<()> {
    let hub_addr = central_net_addr()?;
    log::info!("connecting Central Net (Mk1) hub at: {}", hub_addr);
    let conn = connection::tcp(hub_addr).await?;
    let bcast = super::bcast::BCastService::default();
    let bcast_service_id = <SendBroadcastMessage<serde_json::Value> as RpcMessage>::ID;

    // connect to hub with forwarding handler
    let my_net_node_id = net_node_id(&default_node_id);
    let own_net_node_id = my_net_node_id.clone();
    let call_handler = move |request_id: String, caller: String, addr: String, data: Vec<u8>| {
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
    };

    let event_handler = {
        let bcast = bcast.clone();

        move |topic: String, msg: Vec<u8>| {
            let endpoints = bcast.resolve(&topic);
            let msg: Rc<[u8]> = msg.into();
            Arbiter::spawn(async move {
                for endpoint in endpoints {
                    let addr = format!("{}/{}", endpoint, bcast_service_id);
                    let _ = local_bus::send(addr.as_ref(), "bcast", msg.as_ref()).await;
                }
            })
        }
    };

    let central_bus = connection::connect_with_handler(conn, (call_handler, event_handler));

    // bind my local net service on remote centralised bus under /net/<my_addr>
    for node in &nodes {
        let addr = net_node_id(node);
        central_bus
            .bind(addr)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)))?;
        log::info!("network service bound at: {} as {}", hub_addr, node);
    }

    // bind /net on my local bus and forward all calls to remote bus under /net
    {
        let central_bus = central_bus.clone();
        let source_node_id = default_node_id.to_string();
        local_bus::subscribe(net::BUS_ID, move |_caller: &str, addr: &str, msg: &[u8]| {
            log::debug!(
                "Sending message to hub. Called by: {}, addr: {}.",
                my_net_node_id,
                addr
            );
            // `_caller` here is usually "local", so we replace it with our src node id
            central_bus.call(source_node_id.clone(), addr.to_string(), Vec::from(msg))
        });
    }
    {
        let central_bus = central_bus.clone();

        local_bus::subscribe("/from", move |_: &str, addr: &str, msg: &[u8]| {
            log::debug!("Sending from message to hub. addr: {}.", addr);
            let (from_addr, to_addr, dst) = match parse_from_addr(addr) {
                Ok(v) => v,
                Err(e) => return future::err(Error::GsbBadRequest(e.to_string())).left_future(),
            };
            if !nodes.contains(&from_addr) {
                return future::err(Error::GsbBadRequest(format!(
                    "invalid src node: {:?}",
                    from_addr
                )))
                .left_future();
            }

            let addr = format!("/net/{:?}{}", to_addr, dst);
            central_bus
                .call(from_addr.to_string(), addr, Vec::from(msg))
                .right_future()
        });
    }

    {
        let bcast = bcast.clone();
        let central_bus = central_bus.clone();

        let _ = bus::bind(local_net::BUS_ID, move |subscribe: local_net::Subscribe| {
            let topic = subscribe.topic().to_owned();
            let (is_new, id) = bcast.add(subscribe);
            let central_bus = central_bus.clone();
            async move {
                if is_new {
                    if let Err(e) = central_bus.subscribe(topic.clone()).await {
                        log::error!("fail to subscribe to: {}, {}", topic, e);
                    }
                }
                Ok(id)
            }
        });
    }

    {
        let central_bus = central_bus.clone();
        let addr = format!("{}/{}", local_net::BUS_ID, bcast_service_id);
        let resp: Rc<[u8]> = serde_json::to_vec(&Ok::<(), ()>(())).unwrap().into();
        let _ = local_bus::subscribe(&addr, move |_: &str, _addr: &str, msg: &[u8]| {
            // TODO: remove unwrap here.
            let ent: SendBroadcastMessage<serde_json::Value> = serde_json::from_slice(msg).unwrap();
            let fut = central_bus.broadcast(ent.topic().to_owned(), msg.into());
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

#[cfg(test)]
mod tests {

    use super::*;
    use crate::RemoteEndpoint;
    use ya_core_model::ethaddr::NodeId;

    #[test]
    fn empty() {}

    #[test]
    fn test_gen_parse() {
        let from_id = "0xe93ab94a2095729ad0b7cfa5bfd7d33e1b44d6df"
            .parse::<NodeId>()
            .unwrap();
        let dst = "0x99402605903da83901151b0871ebeae9296ef66b"
            .parse::<NodeId>()
            .unwrap();

        let service = crate::from(from_id).to(dst).service("/public/test/echo");
        let addr = service.addr();
        eprintln!("addr={}", addr);
        let (_parsed_from, _parsed_to, service) = parse_from_addr(addr).unwrap();
        assert_eq!(service, "/test/echo");
    }

    #[test]
    fn test_parse() {
        let out = parse_from_addr("/from/0xe93ab94a2095729ad0b7cfa5bfd7d33e1b44d6df/to/0x99402605903da83901151b0871ebeae9296ef66b");
        assert!(out.is_err())
    }
}
