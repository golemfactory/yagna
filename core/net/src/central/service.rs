use std::cell::RefCell;
use std::net::{SocketAddr, ToSocketAddrs};
use std::rc::Rc;

use futures::channel::oneshot;
use futures::prelude::*;
use tokio::time::{Duration, Instant};
use ya_sb_proto::codec::{GsbMessage, ProtocolError};

use ya_core_model::net;
use ya_core_model::net::local::{BindBroadcastError, SendBroadcastMessage, SendBroadcastStub};
use ya_core_model::net::{local as local_net, net_service};
use ya_core_model::NodeId;
use ya_service_bus::connection::{CallRequestHandler, ClientInfo, ConnectionRef};
use ya_service_bus::{
    connection, serialization, typed as bus, untyped as local_bus, Error, RpcEndpoint, RpcMessage,
};
use ya_utils_networking::resolver;

use crate::bcast::BCastService;
use crate::central::handler::CentralBusHandler;
use crate::central::SUBSCRIPTIONS;
use crate::config::Config;

const CENTRAL_ADDR_ENV_VAR: &str = "CENTRAL_NET_HOST";

async fn central_net_addr() -> std::io::Result<SocketAddr> {
    Ok(match std::env::var(CENTRAL_ADDR_ENV_VAR) {
        Ok(v) => v,
        Err(_) => resolver::resolve_yagna_srv_record("_net._tcp").await?,
    }
    .to_socket_addrs()?
    .next()
    .expect("central net hub addr needed"))
}

/// Initialize net module on a hub.
pub async fn bind_remote(
    client_info: ClientInfo,
    default_node_id: NodeId,
    nodes: Vec<NodeId>,
) -> std::io::Result<oneshot::Receiver<()>> {
    let hub_addr = central_net_addr().await?;
    let conn = connection::tcp(hub_addr).await?;
    let bcast = BCastService::default();
    let bcast_service_id = <SendBroadcastMessage<()> as RpcMessage>::ID;

    // connect to hub with forwarding handler
    let own_net_nodes: Vec<_> = nodes.iter().map(|id| net_service(id)).collect();

    let forward_call = move |request_id: String, caller: String, addr: String, data: Vec<u8>| {
        let prefix = own_net_nodes
            .iter()
            .find(|&own_net_node_id| addr.starts_with(own_net_node_id));
        if let Some(prefix) = prefix {
            // replaces  /net/<dest_node_id>/test/1 --> /public/test/1
            let local_addr: String = addr.replacen(prefix, net::PUBLIC_PREFIX, 1);
            log::trace!(
                "Incoming msg from = {}, to = {}, fwd to local addr = {}, request_id: {}",
                caller,
                addr,
                local_addr,
                request_id
            );
            // actual forwarding to my local bus
            local_bus::call_stream(&local_addr, &caller, &data).right_stream()
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
            let msg: Rc<[u8]> = msg.into();
            let bcast = bcast.clone();

            tokio::task::spawn_local(async move {
                let endpoints = bcast.resolve(&topic).await;
                log::trace!("Received broadcast to topic {} from [{}].", &topic, &caller);
                for endpoint in endpoints {
                    let addr = format!("{}/{}", endpoint, bcast_service_id);
                    log::trace!("Broadcast addr {}", addr);
                    let _ = local_bus::send(addr.as_ref(), &caller, msg.as_ref()).await;
                }
            });
        }
    };

    let (handler, done_rx) = CentralBusHandler::new(forward_call, broadcast_handler);
    let central_bus = connection::connect_with_handler(client_info, conn, handler);

    // bind my local net service(s) on remote centralised bus under /net/<my_identity>
    for node in &nodes {
        let addr = net_service(node);
        central_bus
            .bind(addr.clone())
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)))?;
        log::info!("network service bound at: {} under: {}", hub_addr, addr);
    }

    bind_net_handler(net::BUS_ID, central_bus.clone(), default_node_id);
    bind_net_handler(net::BUS_ID_UDP, central_bus.clone(), default_node_id);
    bind_net_handler(net::BUS_ID_TRANSFER, central_bus.clone(), default_node_id);

    bind_from_handler("/from", central_bus.clone(), nodes.clone());
    bind_from_handler("/udp/from", central_bus.clone(), nodes.clone());
    bind_from_handler("/transfer/from", central_bus.clone(), nodes.clone());

    // Subscribe broadcast on remote
    {
        let bcast = bcast.clone();
        let central_bus = central_bus.clone();

        let _ = bus::bind(local_net::BUS_ID, move |subscribe: local_net::Subscribe| {
            let topic = subscribe.topic().to_owned();
            let bcast = bcast.clone();
            let central_bus = central_bus.clone();
            async move {
                log::debug!("Subscribe topic {} on central bus.", topic);
                let (is_new, id) = bcast.add(subscribe).await;
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
        let resp: Rc<[u8]> = serialization::to_vec(&Ok::<(), ()>(())).unwrap().into();
        let _ = local_bus::subscribe(
            &addr,
            move |caller: &str, _addr: &str, msg: &[u8]| {
                let stub: SendBroadcastStub = match serialization::from_slice(msg) {
                    Ok(m) => m,
                    Err(e) => {
                        return async move {
                            let err = Error::GsbFailure(format!("invalid bcast message: {}", e));
                            Err::<Vec<u8>, _>(err)
                        }
                        .right_future()
                    }
                };

                log::trace!(
                    "Broadcast msg related to topic {} from [{}].",
                    stub.topic,
                    &caller
                );

                let fut = central_bus.broadcast(caller.to_owned(), stub.topic, msg.into());
                let resp = resp.clone();
                async move {
                    if let Err(e) = fut.await {
                        Err(Error::GsbFailure(format!("bcast send failure: {}", e)))
                    } else {
                        Ok(Vec::from(resp.as_ref()))
                    }
                }
                .left_future()
            },
            (),
        );
    }

    Ok(done_rx)
}

fn strip_udp(addr: &str) -> &str {
    // Central NET doesn't support unreliable transport, so we just remove prefix
    // and use reliable protocol.
    match addr.strip_prefix("/udp") {
        None => addr,
        Some(wo_prefix) => wo_prefix,
    }
}

fn bind_net_handler<Transport, H>(
    addr: &str,
    central_bus: ConnectionRef<Transport, H>,
    default_node_id: NodeId,
) where
    Transport: Sink<GsbMessage, Error = ProtocolError>
        + Stream<Item = Result<GsbMessage, ProtocolError>>
        + Unpin
        + 'static,
    H: CallRequestHandler + Unpin + 'static,
{
    // bind /net on my local bus and forward all calls to remote bus under /net
    let log_message = |caller: &str, addr: &str, label: &str| {
        log::trace!(
            "Sending {} message to hub. Called by: {}, addr: {}.",
            label,
            net_service(&caller),
            addr
        );
    };

    // `caller` is usually "local", so we replace it with our default node id
    let central_bus_rpc = central_bus.clone();
    let default_caller_rpc = default_node_id.to_string();
    let rpc = move |_caller: &str, addr: &str, msg: &[u8]| {
        let caller = default_caller_rpc.clone();
        let addr = strip_udp(addr);

        log_message("rpc", &caller, addr);
        let addr = addr.to_string();
        central_bus_rpc
            .call(caller, addr.clone(), Vec::from(msg))
            .map_err(|e| Error::RemoteError(addr, e.to_string()))
    };

    let central_bus_stream = central_bus.clone();
    let default_caller_stream = default_node_id.to_string();
    let stream = move |_caller: &str, addr: &str, msg: &[u8]| {
        let caller = default_caller_stream.clone();
        let addr = strip_udp(addr);

        log_message("stream", &caller, addr);
        let addr = addr.to_string();
        central_bus_stream
            .call_streaming(caller, addr.clone(), Vec::from(msg))
            .map_err(move |e| Error::RemoteError(addr.clone(), e.to_string()))
    };

    local_bus::subscribe(addr, rpc, stream);
}

fn bind_from_handler<Transport, H>(
    addr: &str,
    central_bus: ConnectionRef<Transport, H>,
    nodes: Vec<NodeId>,
) where
    Transport: Sink<GsbMessage, Error = ProtocolError>
        + Stream<Item = Result<GsbMessage, ProtocolError>>
        + Unpin
        + 'static,
    H: CallRequestHandler + Unpin + 'static,
{
    // bind /from/<caller>/to/<addr> on my local bus and forward all calls to remote bus under /net
    let nodes_rpc = nodes.clone();
    let central_bus_rpc = central_bus.clone();
    let rpc = move |_caller: &str, addr: &str, msg: &[u8]| {
        let addr = strip_udp(addr);

        let (from_node, to_addr) = match parse_from_addr(addr) {
            Ok(v) => v,
            Err(e) => return future::err(Error::GsbBadRequest(e.to_string())).left_future(),
        };
        log::trace!("{} is calling (rpc) {}", from_node, to_addr);
        if !nodes_rpc.contains(&from_node) {
            return future::err(Error::GsbBadRequest(format!(
                "caller: {:?} is not on src list: {:?}",
                from_node, nodes_rpc,
            )))
            .left_future();
        }

        central_bus_rpc
            .call(from_node.to_string(), to_addr.clone(), Vec::from(msg))
            .map_err(|e| Error::RemoteError(to_addr, e.to_string()))
            .right_future()
    };

    let nodes_stream = nodes.clone();
    let central_bus_stream = central_bus.clone();
    let stream = move |_caller: &str, addr: &str, msg: &[u8]| {
        let addr = strip_udp(addr);

        let (from_node, to_addr) = match parse_from_addr(addr) {
            Ok(v) => v,
            Err(e) => {
                let err = Error::GsbBadRequest(e.to_string());
                return stream::once(async move { Err(err) })
                    .boxed_local()
                    .left_stream();
            }
        };
        log::trace!("{} is calling (stream) {}", from_node, to_addr);
        if !nodes_stream.contains(&from_node) {
            let err = Error::GsbBadRequest(format!(
                "caller: {:?} is not on src list: {:?}",
                from_node, nodes_stream,
            ));
            return stream::once(async move { Err(err) })
                .boxed_local()
                .left_stream();
        }

        central_bus_stream
            .call_streaming(from_node.to_string(), to_addr, Vec::from(msg))
            .right_stream()
    };

    local_bus::subscribe(addr, rpc, stream);
}

async fn unbind_remote(nodes: Vec<NodeId>) {
    let addrs = nodes
        .into_iter()
        .map(|node_id| net_service(node_id))
        .chain(std::iter::once(format!(
            "{}/{}",
            local_net::BUS_ID,
            <SendBroadcastMessage<()> as RpcMessage>::ID
        )))
        .chain(
            [net::BUS_ID, local_net::BUS_ID, "/from"]
                .iter()
                .map(|s| s.to_string()),
        )
        .collect::<Vec<_>>();

    log::debug!("Unbinding remote handlers");
    for addr in addrs {
        if let Err(e) = bus::unbind(addr.as_str()).await {
            log::error!("Unable to unbind {}: {:?}", addr, e);
        }
    }
}

async fn resubscribe() {
    futures::stream::iter({ SUBSCRIPTIONS.lock().unwrap().clone() }.into_iter())
        .for_each(|msg| {
            let topic = msg.topic().to_owned();
            async move {
                Ok::<_, BindBroadcastError>(bus::service(net::local::BUS_ID).send(msg).await??)
            }
            .map_err(move |e| log::error!("Failed to subscribe {}: {}", topic, e))
            .then(|_| futures::future::ready(()))
        })
        .await;
}

pub(crate) async fn rebind<B, U, Fb, Fu, Fr, E>(
    reconnect: Rc<RefCell<ReconnectContext>>,
    mut bind: B,
    unbind: Rc<RefCell<U>>,
) -> anyhow::Result<()>
where
    B: FnMut() -> Fb + 'static,
    U: FnMut() -> Fu + 'static,
    Fb: Future<Output = std::io::Result<Fr>> + 'static,
    Fu: Future<Output = ()> + 'static,
    Fr: Future<Output = Result<(), E>> + 'static,
    E: 'static,
{
    let (tx, rx) = oneshot::channel();
    let unbind_clone = unbind.clone();

    loop {
        match bind().await {
            Ok(dc_rx) => {
                if let Some(start) = reconnect.borrow_mut().last_disconnect {
                    let end = Instant::now();
                    metrics::timing!("net.reconnect.time", start, end);
                }
                reconnect.replace(Default::default());
                metrics::counter!("net.connect", 1);

                let reconnect_clone = reconnect.clone();
                tokio::task::spawn_local(async move {
                    if let Ok(_) = dc_rx.await {
                        metrics::counter!("net.disconnect", 1);
                        reconnect_clone.borrow_mut().last_disconnect = Some(Instant::now());
                        log::warn!("Handlers disconnected");
                        (*unbind_clone.borrow_mut())().await;
                        let _ = tx.send(());
                    }
                });
                break;
            }
            Err(error) => {
                let delay = { reconnect.borrow_mut().next().unwrap() };
                log::warn!(
                    "Failed to bind handlers: {}; retrying in {} s",
                    error,
                    delay.as_secs_f32()
                );
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        return Err(anyhow::anyhow!("Net initialization interrupted"));
                    },
                    _ = tokio::time::sleep(delay) => {},
                }
            }
        }
    }

    tokio::task::spawn_local(
        rx.then(move |_| rebind(reconnect, bind, unbind).then(|_| futures::future::ready(()))),
    );
    Ok(())
}

pub(crate) struct ReconnectContext {
    pub current: f32, // s
    pub max: f32,     // s
    pub factor: f32,
    pub last_disconnect: Option<Instant>,
}

impl Iterator for ReconnectContext {
    type Item = Duration;

    fn next(&mut self) -> Option<Self::Item> {
        self.current = self.max.min(self.current * self.factor);
        Some(Duration::from_secs_f32(self.current))
    }
}

impl Default for ReconnectContext {
    fn default() -> Self {
        ReconnectContext {
            current: 1.,
            max: 1800.,
            factor: 2.,
            last_disconnect: None,
        }
    }
}

pub struct Net;

impl Net {
    pub async fn gsb<Context>(_: Context, _config: Config) -> anyhow::Result<()> {
        let (default_id, ids) = crate::service::identities().await?;
        log::info!(
            "CENTRAL_NET - Using default identity as network id: {:?}",
            default_id
        );

        let client_info = ClientInfo::new("sb-client-net");
        let ids_clone = ids.clone();

        let bind = move || {
            let client_info = client_info.clone();
            let ids = ids.clone();
            async move {
                let rx = bind_remote(client_info.clone(), default_id, ids.clone()).await?;
                resubscribe().await;
                Ok(rx)
            }
        };
        let unbind = Rc::new(RefCell::new(move || unbind_remote(ids_clone.clone())));

        rebind(Default::default(), bind, unbind).await?;
        Ok(())
    }
}

pub(crate) fn parse_from_addr(from_addr: &str) -> anyhow::Result<(NodeId, String)> {
    let mut it = from_addr.split("/").fuse();
    if let (Some(""), Some("from"), Some(from_node_id), Some("to"), Some(to_node_id)) =
        (it.next(), it.next(), it.next(), it.next(), it.next())
    {
        to_node_id.parse::<NodeId>()?;
        let prefix = 10 + from_node_id.len();
        let service_id = &from_addr[prefix..];
        if let Some(_) = it.next() {
            return Ok((from_node_id.parse()?, net_service(service_id)));
        }
    }
    anyhow::bail!("invalid net-from destination: {}", from_addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ya_core_model::net::RemoteEndpoint;

    #[test]
    fn parse_generated_from_to_service_should_pass() {
        let from_id = "0xe93ab94a2095729ad0b7cfa5bfd7d33e1b44d6df"
            .parse::<NodeId>()
            .unwrap();
        let dst = "0x99402605903da83901151b0871ebeae9296ef66b"
            .parse::<NodeId>()
            .unwrap();

        let remote_service = ya_core_model::net::from(from_id)
            .to(dst)
            .service("/public/test/echo");
        let addr = remote_service.addr();
        eprintln!("from/to service address: {}", addr);
        let (parsed_from, parsed_to) = parse_from_addr(addr).unwrap();
        assert_eq!(parsed_from, from_id);
        assert_eq!(
            parsed_to,
            "/net/0x99402605903da83901151b0871ebeae9296ef66b/test/echo"
        );
    }

    #[test]
    fn parse_no_service_should_fail() {
        let out = parse_from_addr("/from/0xe93ab94a2095729ad0b7cfa5bfd7d33e1b44d6df/to/0x99402605903da83901151b0871ebeae9296ef66b");
        assert!(out.is_err())
    }

    #[test]
    fn parse_with_service_should_pass() {
        let out = parse_from_addr("/from/0xe93ab94a2095729ad0b7cfa5bfd7d33e1b44d6df/to/0x99402605903da83901151b0871ebeae9296ef66b/x");
        assert!(out.is_ok())
    }
}
