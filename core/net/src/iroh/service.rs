use std::future::Future;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::task::Poll;

use anyhow::{anyhow, Context as AnyhowContext};
use futures::channel::{mpsc, oneshot};
use futures::{FutureExt, SinkExt, StreamExt, TryFutureExt, TryStreamExt};
use metrics::counter;
use tokio::sync::RwLock;

use ya_core_model::identity::event::IdentityEvent;
use ya_core_model::net::local::{
    BindBroadcastError, BroadcastMessage, NewNeighbour, SendBroadcastMessage, SendBroadcastStub,
};
use ya_core_model::{identity, net, NodeId};
use ya_relay_client::channels::ForwardSender;
use ya_relay_client::crypto::CryptoProvider;
use ya_relay_client::model::{Payload, TransportType};
use ya_relay_client::{Client, GenericSender};
use ya_sb_proto::codec::GsbMessage;
use ya_sb_proto::CallReplyCode;
use ya_service_bus::untyped::{Fn4HandlerExt, Fn4StreamHandlerExt};
use ya_service_bus::{
    serialization, typed, untyped as local_bus, Error, ResponseChunk, RpcEndpoint, RpcMessage, serialization::to_vec,
};

use crate::bcast::BCastService;
use crate::config::Config;
use crate::iroh::codec;
use crate::iroh::codec::encode_message;
use crate::iroh::crypto::IdentityCryptoProvider;
use crate::service::NET_TYPE;
use crate::{broadcast, NetType};

use iroh::net::{
    relay::RelayMode,
    key::{PublicKey, SecretKey},
    Endpoint,
    NodeId as IrohId, NodeAddr,
};

type BusSender = mpsc::Sender<ResponseChunk>;
type BusReceiver = mpsc::Receiver<ResponseChunk>;
type NetSender = mpsc::Sender<Payload>;
type NetSinkKind = ForwardSender;
type NetSinkKey = (NodeId, TransportType);

lazy_static::lazy_static! {
    pub(crate) static ref BCAST: BCastService = Default::default();
    pub(crate) static ref SHUTDOWN_TX: Arc<RwLock<Option<oneshot::Sender<()>>>> = Default::default();
}

pub struct Net;

impl Net {
    pub async fn gsb<Context>(_: Context, _config: Config) -> anyhow::Result<()> {
        ya_service_bus::serialization::CONFIG.set_compress(true);

        let (default_id, ids) = crate::service::identities().await?;
        let (started_tx, started_rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let iroh_key = SecretKey::generate();
        let iroh_id = iroh_key.public();

        log::info!(
            "Iroh NET - Using default identity as network id: {}",
            iroh_id,
        );

        std::thread::spawn(move || {
            let system = actix::System::new();
            system.block_on(async move {
                SHUTDOWN_TX.write().await.replace(shutdown_tx);

                let result = start_network(default_id, ids, iroh_key).await;
                started_tx.send(result).expect("Unable to start network");
                let _ = shutdown_rx.await;
            });
        });

        started_rx
            .await
            .map_err(|_| anyhow!("Error starting network"))?
    }

    pub async fn shutdown() -> anyhow::Result<()> {
        // TODO: kill above thread
        if let Some(sender) = { SHUTDOWN_TX.write().await.take() } {
            let _ = sender.send(());
        }
        // TODO: close iroh channel/client
        Ok(())
    }
}

pub async fn start_network(
    default_id: NodeId,
    _ids: Vec<NodeId>,
    iroh_key: SecretKey,
) -> anyhow::Result<()> {
    counter!("net.connections.p2p", 0);
    counter!("net.connections.relay", 0);

    //let broadcast_size = (config.broadcast_size, config.pub_broadcast_size);
    //let crypto = IdentityCryptoProvider::new(default_id);
    let client = build_client(iroh_key).await?;

    super::cli::bind_service();

    let net_handler = || {
        move |addr: &str| match parse_net_to_addr(addr) {
            Ok((to, addr)) => Ok((default_id, to, addr)),
            Err(err) => anyhow::bail!("invalid address: {}", err),
        }
    };

    bind_local_bus(
        client.clone(),
        net::BUS_ID,
        TransportType::Reliable,
        net_handler(),
    );

    tokio::task::spawn_local(listen(client.clone()));

    /*
    bind_local_bus(
        client.clone(),
        net::BUS_ID_UDP,
        TransportType::Unreliable,
        net_handler(),
    );
    bind_local_bus(
        client.clone(),
        net::BUS_ID_TRANSFER,
        TransportType::Transfer,
        net_handler(),
    );

    let from_handler = || {
        move |addr: &str| match parse_from_to_addr(addr) {
            Ok((from, to, addr)) => Ok((from, to, addr)),
            Err(err) => anyhow::bail!("invalid address: {}", err),
        }
    };

    bind_local_bus(
        client.clone(),
        "/from",
        TransportType::Reliable,
        from_handler(),
    );
    bind_local_bus(
        client.clone(),
        "/udp/from",
        TransportType::Unreliable,
        from_handler(),
    );
    bind_local_bus(
        client.clone(),
        "/transfer/from",
        TransportType::Transfer,
        from_handler(),
    );

    bind_broadcast_handlers(client.clone(), broadcast_size);
    bind_identity_event_handler(client.clone(), crypto).await;
    */

    Ok(())
}

async fn build_client(iroh_key: SecretKey) -> anyhow::Result<Endpoint> {
    Endpoint::builder()
        .secret_key(iroh_key)
        .alpns(vec![b"golem".to_vec()])
        .relay_mode(RelayMode::Default)
        .bind()
        .await
}

fn bind_local_bus<F>(
    base_client: Endpoint,
    address: &'static str,
    transport: TransportType,
    resolver: F,
) where
    F: Fn(&str) -> anyhow::Result<(NodeId, IrohId, String)> + 'static,
{
    let resolver = Rc::new(resolver);
    let resolver_ = resolver.clone();

    let client = base_client.clone();
    let rpc = move |caller: &str, addr: &str, msg: &[u8], no_reply: bool| {
        let (caller_id, remote_id, address) = match (*resolver_)(addr) {
            Ok(id) => id,
            Err(err) => {
                log::error!("rpc {} forward error: {}", addr, err);
                return async move { Err(Error::GsbFailure(err.to_string())) }.left_future();
            }
        };

        log::error!(
            "TEST local bus: rpc call (egress): {} ({} -> {}), no_reply: {no_reply}",
            address,
            caller_id,
            remote_id,
        );

        let is_local_dest = remote_id == client.node_id();

        let rx = if no_reply {
            if is_local_dest {
                push_bus_to_local(caller_id, addr, msg);
            } else {
                push_bus_to_net(
                    client.clone(),
                    caller_id,
                    remote_id,
                    address,
                    msg,
                    transport,
                );
            }

            None
        } else {
            let rx = if is_local_dest {
                let (tx, rx) = mpsc::channel(1);
                forward_bus_to_local(caller_id, address.as_str(), msg, client.clone(), tx);
                rx
            } else {
                forward_bus_to_net(
                    client.clone(),
                    caller_id,
                    remote_id,
                    address,
                    msg,
                    transport,
                )
            };

            Some(rx)
        };

        async move {
            match rx {
                None => Ok(Vec::new()),
                Some(mut rx) => match rx.next().await.ok_or(Error::Cancelled) {
                    Ok(chunk) => match chunk {
                        ResponseChunk::Full(data) => codec::decode_reply(data),
                        ResponseChunk::Part(_) => {
                            Err(Error::GsbFailure("partial response".to_string()))
                        }
                    },
                    Err(err) => Err(err),
                },
            }
        }
        .right_future()
    };

    let client = base_client;
    let stream = move |caller: &str, addr: &str, msg: &[u8], no_reply: bool| {
        let (caller_id, remote_id, address) = match (*resolver)(addr) {
            Ok(id) => id,
            Err(err) => {
                log::error!("local bus: stream call (egress) to {} error: {}", addr, err);
                return futures::stream::once(
                    async move { Err(Error::GsbFailure(err.to_string())) },
                )
                .boxed_local()
                .left_stream();
            }
        };

        log::error!(
            "TEST local bus: stream call (egress): {} ({} -> {})",
            address,
            caller_id,
            remote_id
        );

        let is_local_dest = remote_id == client.node_id();
        let rx = if no_reply {
            if is_local_dest {
                push_bus_to_local(caller_id, addr, msg);
            } else {
                push_bus_to_net(
                    client.clone(),
                    caller_id,
                    remote_id,
                    address,
                    msg,
                    transport,
                );
            }
            futures::stream::empty().left_stream()
        } else {
            let rx = if is_local_dest {
                let (tx, rx) = mpsc::channel(1);
                forward_bus_to_local(caller_id, addr, msg, client.clone(), tx);
                rx
            } else {
                forward_bus_to_net(
                    client.clone(),
                    caller_id,
                    remote_id,
                    address,
                    msg,
                    transport,
                )
            };
            rx.right_stream()
        };

        let eos = Rc::new(AtomicBool::new(false));
        let eos_chain = eos.clone();

        rx.map(move |chunk| match chunk {
            ResponseChunk::Full(v) => {
                eos.store(true, Relaxed);
                codec::decode_reply(v).map(ResponseChunk::Full)
            }
            chunk => Ok(chunk),
        })
        .chain(futures::stream::poll_fn(move |_| {
            if eos_chain.load(Relaxed) {
                Poll::Ready(None)
            } else {
                eos_chain.store(true, Relaxed);
                Poll::Ready(Some(Ok(ResponseChunk::Full(Vec::new()))))
            }
        }))
        .boxed_local()
        .right_stream()
    };

    log::debug!("local bus: subscribing to {}", address);
    let rpc = rpc.into_handler();
    let stream = stream.into_stream_handler();
    local_bus::subscribe(address, rpc, stream);
}

/// Handle identity changes
async fn bind_identity_event_handler(client: Endpoint, crypto: IdentityCryptoProvider) {
    todo!()
}

/// Forward requests from and to the local bus
fn forward_bus_to_local(caller: NodeId, addr: &str, data: &[u8], client: Endpoint, tx: BusSender) {
    let address = addr.replacen(&format!("/net/{}", client.node_id()), net::PUBLIC_PREFIX, 1);
    log::error!("TEST forward to local {caller} to {addr} -> {address}");
    local_bus::call_stream(&address, &caller.to_string(), data);
}

fn push_bus_to_local(caller: NodeId, addr: &str, data: &[u8]) {
    todo!()
}

/// Forward requests from local bus to the network
fn forward_bus_to_net(
    client: Endpoint,
    caller_id: NodeId,
    remote_id: IrohId,
    address: impl ToString,
    msg: &[u8],
    transport: TransportType,
) -> BusReceiver {
    let address = address.to_string();
    let request_id = gen_id().to_string();

    ya_packet_trace::packet_trace_maybe!("net::forward_bus_to_net", {
        ya_packet_trace::try_extract_from_ip_frame(msg)
    });

    let (tx, rx) = mpsc::channel(1);
    let msg = match codec::encode_request(
        caller_id,
        address.clone(),
        request_id.clone(),
        msg.to_vec(),
        false,
    ) {
        Ok(vec) => vec,
        Err(err) => {
            log::debug!("Forward bus->net ({caller_id} -> {remote_id}), address: {address}: invalid request: {err}");
            handler_reply_bad_request(request_id, format!("Net: invalid request: {err}"), tx);
            return rx;
        }
    };

    let mut request = Request {
        caller_id,
        remote_id,
        address: address.clone(),
        tx: tx.clone(),
    };
    let response: Result<(), ()> = Ok(());
    tokio::task::spawn_local(async move {
        request.tx.send(ResponseChunk::Full(to_vec(&response).unwrap())).await;
    });

    tokio::task::spawn_local(async move {
        log::error!(
            "TEST Local bus handler ({caller_id} -> {remote_id}), address: {address}, id: {request_id} -> send message to remote ({} B)",
            msg.len()
        );

        log::error!("TEST a1");
        let relay_url = client.watch_home_relay().next().await;
        log::error!("TEST a2");
        let addr = NodeAddr::from_parts(remote_id, relay_url, vec![]);
        log::error!("TEST a3");
        let conn = client.connect(addr, b"golem").await.unwrap();
        log::error!("TEST a4");
        let (mut send, _) = conn.open_bi().await.unwrap();
        log::error!("TEST a5");
        send.write_all(&msg).await.unwrap();
        log::error!("TEST a6");
        send.finish().unwrap();
        log::error!("TEST a7");
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        conn.close(0u32.into(), &[]);
        log::error!("TEST a8");
    });

    rx
}

fn push_bus_to_net(
    client: Endpoint,
    caller_id: NodeId,
    remote_id: IrohId,
    address: impl ToString,
    msg: &[u8],
    transport: TransportType,
) {
    let address = address.to_string();
    let request_id = gen_id().to_string();

    ya_packet_trace::packet_trace_maybe!("net::forward_bus_to_net", {
        ya_packet_trace::try_extract_from_ip_frame(msg)
    });

    let msg = match codec::encode_request(
        caller_id,
        address.clone(),
        request_id.clone(),
        msg.to_vec(),
        true,
    ) {
        Ok(vec) => vec,
        Err(err) => {
            log::debug!("Push bus->net ({caller_id} -> {remote_id}), address: {address}: invalid request: {err}");
            return;
        }
    };

    tokio::task::spawn_local(async move {
        log::debug!(
            "Local bus push handler ({caller_id} -> {remote_id}), address: {address}, id: {request_id} -> send message to remote ({} B)",
            msg.len()
        );

        // TODO: send
    });
}

/// Forward broadcast messages from the local bus to the network
fn broadcast_handler(
    client: Endpoint,
    caller: &str,
    _addr: &str,
    msg: &[u8],
    broadcast_size: (u32, u32),
) {
    todo!()
}

fn bind_broadcast_handlers(client: Endpoint, broadcast_size: (u32, u32)) {
    todo!()
}

/// Handle incoming forward messages
async fn listen(client: Endpoint) {
    while let Some(incoming) = client.accept().await {
        let connecting = match incoming.accept() {
            Ok(connecting) => connecting,
            Err(err) => {
                log::error!("Incoming connection failed: {:#}", err);
                continue;
            }
        };

        let connection = connecting.await.unwrap();
        let remote_id = iroh::net::endpoint::get_remote_node_id(&connection).unwrap();
        log::error!("TEST connection from {}", remote_id);

        let client = client.clone();
        tokio::task::spawn_local(async move {
            log::error!("TEST 1");
            let (_, mut recv) = connection.accept_bi().await?;
            log::error!("TEST 2");
            let payload = recv.read_to_end(65535).await?;
            log::error!("TEST recv msg");
            match codec::decode_message(&payload) {
                Ok(Some(GsbMessage::CallRequest(request))) => {
                    if request.no_reply {
                        handle_push(request, remote_id);
                        Ok(())
                    } else {
                        handle_request(client, request, remote_id);
                        Ok(())
                    }
                }
                Ok(Some(GsbMessage::CallReply(reply))) => {
                    handle_reply(reply, remote_id);
                    Ok(())
                }
                Ok(Some(GsbMessage::BroadcastRequest(request))) => {
                    handle_broadcast(request, remote_id);
                    Ok(())
                }
                Ok(Some(_)) => anyhow::bail!("unexpected message type"),
                Ok(None) => {
                    log::trace!("Received a partial message from {remote_id}");
                    Ok(())
                }
                Err(err) => anyhow::bail!("Received message error: {}", err),
            }
        });
    }
}

/// Forward messages from the network to the local bus
fn handle_push(
    request: ya_sb_proto::CallRequest,
    remote_id: IrohId,
) {
    let caller_id = NodeId::from_str(&request.caller).ok();

    let address = request.address;
    let request_id = request.request_id;
    let caller_id = caller_id.unwrap();

    log::error!("TEST Handle push request {request_id} to {address} from {remote_id}");
    let mut services = std::collections::HashSet::new();
    services.insert(net::net_service_udp(remote_id));
    services.insert(net::net_service(remote_id));
    services.insert(net::net_transfer_service(remote_id));
    let x = ya_sb_util::RevPrefixes(&address)
        .find_map(|s| services.get(s))
        .map(|s| address.replacen(s, net::PUBLIC_PREFIX, 1));
    log::error!("TEST {:?}", x);
    //local_bus::push(&address, &request.caller, &request.data)
}

/// Forward messages from the network to the local bus
fn handle_request(
    client: Endpoint,
    request: ya_sb_proto::CallRequest,
    remote_id: IrohId,
) {
    let caller_id = NodeId::from_str(&request.caller).ok();

    let address = request.address;
    let caller_id = caller_id.unwrap();
    let request_id = request.request_id;

    let addr = address.replacen(&format!("/net/{}", client.node_id()), net::PUBLIC_PREFIX, 1);
    log::error!("TEST Handle request {request_id}: {caller_id} to {address} -> {addr} from {remote_id}: {:?}", request.data);

    let eos = Rc::new(AtomicBool::new(false));
    let eos_map = eos.clone();

    log::error!("TEST cmp {} {} {:?}", addr, request.caller, request.data);
    let mut stream = local_bus::call_stream(&addr, &request.caller, &request.data);
    tokio::task::spawn_local(async move {
        while let Some(item) = stream.next().await {
        }
    });
}

/// Forward replies from the network to the local bus
fn handle_reply(
    reply: ya_sb_proto::CallReply,
    remote_id: IrohId,
) {
    let full = reply.reply_type == ya_sb_proto::CallReplyType::Full as i32;

    log::error!(
        "TEST Handle reply from node {remote_id} (full: {full}, code: {}, id: {}) {} B",
        reply.code,
        reply.request_id,
        reply.data.len(),
    );
}

/// Forward broadcasts from the network to the local bus
fn handle_broadcast(
    request: ya_sb_proto::BroadcastRequest,
    remote_id: IrohId,
) {
    let caller_id = NodeId::from_str(&request.caller).ok();

    log::error!(
        "TEST Received broadcast to topic {} from [{}].",
        &request.topic,
        &request.caller
    );
}

#[derive(Clone)]
struct Request<S: Clone> {
    #[allow(unused)]
    caller_id: NodeId,
    #[allow(unused)]
    remote_id: IrohId,
    #[allow(unused)]
    address: String,
    tx: S,
}

#[inline]
fn handler_reply_bad_request(request_id: impl ToString, error: impl ToString, tx: BusSender) {
    handler_reply_err(request_id, error, CallReplyCode::CallReplyBadRequest, tx);
}

#[inline]
fn handler_reply_service_err(request_id: impl ToString, error: impl ToString, tx: BusSender) {
    handler_reply_err(request_id, error, CallReplyCode::ServiceFailure, tx);
}

fn handler_reply_err(
    request_id: impl ToString,
    error: impl ToString,
    code: impl Into<i32>,
    mut tx: BusSender,
) {
    let err = codec::encode_error(request_id, error, code.into()).unwrap();
    tokio::task::spawn_local(async move {
        let _ = tx.send(ResponseChunk::Full(err)).await;
    });
}

pub fn parse_net_to_addr(addr: &str) -> anyhow::Result<(IrohId, String)> {
    const ADDR_CONST: usize = 6;

    let mut it = addr.split('/').fuse().skip(1).peekable();
    let (prefix, to) = match (it.next(), it.next(), it.next()) {
        (Some("udp"), Some("net"), Some(to)) if it.peek().is_some() => ("/udp", to),
        (Some("net"), Some(to), Some(_)) => ("", to),
        (Some("transfer"), Some("net"), Some(to)) if it.peek().is_some() => ("/transfer", to),
        _ => anyhow::bail!("invalid net-to destination: {}", addr),
    };

    let to_id = to.parse::<IrohId>()?;
    let skip = prefix.len() + ADDR_CONST + to.len();
    let addr = net::net_service(format!("{}/{}", to, &addr[skip..]));

    Ok((to_id, format!("{}{}", prefix, addr)))
}

pub fn parse_from_to_addr(addr: &str) -> anyhow::Result<(NodeId, NodeId, String)> {
    const ADDR_CONST: usize = 10;

    let mut it = addr.split('/').fuse().skip(1).peekable();
    let (prefix, from, to) = match (it.next(), it.next(), it.next(), it.next(), it.next()) {
        (Some("udp"), Some("from"), Some(from), Some("to"), Some(to)) if it.peek().is_some() => {
            ("/udp", from, to)
        }
        (Some("from"), Some(from), Some("to"), Some(to), Some(_)) => ("", from, to),
        (Some("transfer"), Some("from"), Some(from), Some("to"), Some(to))
            if it.peek().is_some() =>
        {
            ("/transfer", from, to)
        }
        _ => anyhow::bail!("invalid net-from-to destination: {}", addr),
    };

    let from_id = from.parse::<NodeId>()?;
    let to_id = to.parse::<NodeId>()?;
    let skip = prefix.len() + ADDR_CONST + from.len();
    let addr = net::net_service(&addr[skip..]);

    Ok((from_id, to_id, format!("{}{}", prefix, addr)))
}

fn gen_id() -> u64 {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    rng.gen::<u64>() & 0x001f_ffff_ffff_ffff_u64
}
