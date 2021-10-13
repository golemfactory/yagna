use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, ToSocketAddrs};
use std::pin::Pin;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use anyhow::Context as AnyhowContext;
use futures::channel::mpsc;
use futures::{FutureExt, SinkExt, Stream, StreamExt, TryStreamExt};
use tokio::time::{self, Duration};
use url::Url;

use ya_core_model::net::{self, net_service};
use ya_core_model::NodeId;
use ya_net_server::testing::{Client, ClientBuilder};
use ya_relay_proto::codec::forward::{PrefixedSink, PrefixedStream, SinkKind};
use ya_sb_proto::codec::{GsbMessage, ProtocolError};
use ya_sb_proto::CallReplyCode;
use ya_service_bus::{untyped as local_bus, Error, ResponseChunk};
use ya_utils_networking::resolver;

use crate::bcast::BCastService;
use crate::hybrid::crypto::IdentityCryptoProvider;

const NET_RELAY_HOST_ENV_VAR: &str = "NET_RELAY_HOST";
const DEFAULT_NET_RELAY_HOST: &str = "127.0.0.1:7464";
const DEFAULT_BROADCAST_NODE_COUNT: u32 = 12;
const DEFAULT_PING_INTERVAL: Duration = Duration::from_millis(15000);

pub type BCastHandler = Box<dyn FnMut(String, &[u8]) + Send>;

type BusSender = mpsc::Sender<ResponseChunk>;
type BusReceiver = mpsc::Receiver<ResponseChunk>;
type NetSender = mpsc::Sender<Vec<u8>>;
type NetReceiver = mpsc::Receiver<Vec<u8>>;
type NetSinkKind = SinkKind<NetSender, mpsc::SendError>;
type NetSinkKey = (NodeId, bool);

type ArcMap<K, V> = Arc<Mutex<HashMap<K, V>>>;

lazy_static::lazy_static! {
    pub(crate) static ref BCAST: BCastService = Default::default();
    pub(crate) static ref BCAST_HANDLERS: ArcMap<String, Arc<Mutex<BCastHandler>>> = Default::default();
    pub(crate) static ref BCAST_SENDER: Arc<Mutex<Option<NetSender>>> = Default::default();
}

thread_local! {
    static CLIENT: RefCell<Option<Client>> = Default::default();
}

async fn relay_addr() -> std::io::Result<SocketAddr> {
    Ok(match std::env::var(NET_RELAY_HOST_ENV_VAR) {
        Ok(val) => val,
        Err(_) => resolver::resolve_yagna_srv_record("_net_relay._udp")
            .await
            // FIXME: remove
            .unwrap_or_else(|_| DEFAULT_NET_RELAY_HOST.to_string()),
    }
    .to_socket_addrs()?
    .next()
    .expect("relay address needed"))
}

pub struct Net;

impl Net {
    pub async fn gsb<Context>(_: Context) -> anyhow::Result<()> {
        let (default_id, ids) = crate::service::identities().await?;
        start_network(default_id, ids).await?;
        Ok(())
    }
}

pub async fn start_network(default_id: NodeId, ids: Vec<NodeId>) -> anyhow::Result<()> {
    let url = Url::parse(&format!("udp://{}", relay_addr().await?))?;
    let provider = IdentityCryptoProvider::new(default_id);

    log::info!("starting network (hybrid) with identity: {}", default_id);

    let client = ClientBuilder::from_url(url)
        .crypto(provider)
        .connect()
        .build()
        .await?;
    CLIENT.with(|c| {
        c.borrow_mut().replace(client.clone());
    });

    let (btx, brx) = mpsc::channel(1);
    BCAST_SENDER.lock().unwrap().replace(btx);

    let receiver = client.forward_receiver().await.unwrap();
    let services = ids.iter().map(|id| net_service(id)).collect();
    let state = State::new(ids, services);

    // outbound traffic
    let state_ = state.clone();
    spawn_local_bus_handler(net::BUS_ID, state.clone(), move |_, addr| {
        let from_node = default_id.clone();
        let (to_node, addr) = match parse_net_to_addr(addr) {
            Ok(id) => id,
            Err(err) => anyhow::bail!("invalid address: {}", err),
        };

        if !state_.inner.borrow().ids.contains(&from_node) {
            anyhow::bail!("unknown identity: {:?}", from_node);
        }
        Ok((from_node, to_node, addr))
    });

    let state_ = state.clone();
    spawn_local_bus_handler("/from", state.clone(), move |_, addr| {
        let (from_node, to_node, addr) = match parse_from_to_addr(addr) {
            Ok(tup) => tup,
            Err(err) => anyhow::bail!("invalid address: {}", err),
        };

        if !state_.inner.borrow().ids.contains(&from_node) {
            anyhow::bail!("unknown identity: {:?}", from_node);
        }
        Ok((from_node, to_node, addr))
    });

    spawn_broadcast_handler(brx);

    // inbound traffic
    let state_ = state.clone();
    tokio::task::spawn_local(receiver.for_each(move |fwd| {
        let state = state_.clone();
        async move {
            let key = (fwd.node_id, fwd.reliable);
            let mut tx = match {
                let inner = state.inner.borrow();
                inner.routes.get(&key).cloned()
            } {
                Some(cached) => cached,
                None => {
                    let (tx, rx) = mpsc::channel(1);
                    {
                        let mut inner = state.inner.borrow_mut();
                        inner.routes.insert(key, tx.clone());
                    }

                    let rx = if fwd.reliable {
                        PrefixedStream::new(rx)
                            .inspect_err(|e| log::debug!("prefixed stream error: {}", e))
                            .filter_map(|r| async move { r.ok().map(|b| b.to_vec()) })
                            .boxed_local()
                    } else {
                        rx.boxed_local()
                    };

                    spawn_inbound_handler(rx, fwd.node_id, fwd.reliable, state.clone());
                    tx
                }
            };

            log::trace!("received forward packet ({} B)", fwd.payload.len());

            if tx.send(fwd.payload.into()).await.is_err() {
                log::debug!("net routing error: channel closed");
                let mut inner = state.inner.borrow_mut();
                inner.routes.remove(&key);
                inner.forward.remove(&key);
            }
        }
    }));

    // Keep server connection alive by pinging every `DEFAULT_PING_INTERVAL` seconds.
    let client_ = client.clone();
    tokio::task::spawn_local(async move {
        let mut interval = time::interval(DEFAULT_PING_INTERVAL);
        loop {
            interval.tick().await;
            if let Ok(session) = client_.server_session().await {
                log::trace!("Sending ping to keep session alive.");
                let _ = session.ping().await;
            }
        }
    });

    Ok(())
}

fn spawn_local_bus_handler<F>(address: &'static str, state: State, resolver: F)
where
    F: Fn(&str, &str) -> anyhow::Result<(NodeId, NodeId, String)> + 'static,
{
    fn reply_bad_request(request_id: impl ToString, error: impl ToString, tx: BusSender) {
        reply_err(request_id, error, CallReplyCode::CallReplyBadRequest, tx);
    }

    fn reply_service_err(request_id: impl ToString, error: impl ToString, tx: BusSender) {
        reply_err(request_id, error, CallReplyCode::ServiceFailure, tx);
    }

    fn reply_err(
        request_id: impl ToString,
        error: impl ToString,
        code: impl Into<i32>,
        mut tx: BusSender,
    ) {
        let err = encode_error(request_id, error, code.into()).unwrap();
        tokio::task::spawn_local(async move {
            let _ = tx.send(ResponseChunk::Full(err)).await;
        });
    }

    fn forward_net(
        caller_id: NodeId,
        remote_id: NodeId,
        address: impl ToString,
        msg: &[u8],
        state: &State,
    ) -> BusReceiver {
        let address = address.to_string();
        let state = state.clone();
        let request_id = gen_id().to_string();

        log::trace!("forward net {}", address);

        let (tx, rx) = mpsc::channel(1);
        let msg = match encode_request(caller_id, address.clone(), request_id.clone(), msg.to_vec())
        {
            Ok(vec) => vec,
            Err(err) => {
                log::debug!("forward net: invalid request: {}", err);
                reply_bad_request(request_id, format!("invalid request: {}", err), tx);
                return rx;
            }
        };

        {
            let mut inner = state.inner.borrow_mut();
            inner.requests.insert(
                request_id.clone(),
                Request {
                    caller_id,
                    remote_id,
                    address,
                    tx: tx.clone(),
                },
            );
        }

        tokio::task::spawn_local(async move {
            log::trace!(
                "local bus handler -> send message to remote ({} B)",
                msg.len()
            );

            match state.forward_sink(remote_id, true).await {
                Ok(mut session) => {
                    let _ = session.send(msg).await.map_err(|_| {
                        let err = format!("error sending message: session closed");
                        reply_service_err(request_id, err, tx);
                    });
                }
                Err(error) => {
                    let err = format!("error forwarding message: {}", error);
                    reply_service_err(request_id, err, tx);
                }
            };
        });

        rx
    }

    fn forward_local(caller: &str, addr: &str, data: &[u8], state: &State, tx: BusSender) {
        let address = match {
            let inner = state.inner.borrow();
            inner
                .services
                .iter()
                .find(|&id| addr.starts_with(id))
                // replaces  /net/<dest_node_id>/test/1 --> /public/test/1
                .map(|s| addr.replacen(s, net::PUBLIC_PREFIX, 1))
        } {
            Some(address) => address,
            None => {
                let err = format!("unknown address: {}", addr);
                reply_bad_request("unknown", err, tx);
                return;
            }
        };

        log::trace!("forwarding /net call to a local endpoint: {}", address);

        let send = local_bus::call_stream(address.as_str(), caller, data);
        tokio::task::spawn_local(async move {
            let _ = send
                .map_err(|e| Error::GsbFailure(e.to_string()))
                .forward(tx.sink_map_err(|e| Error::GsbFailure(e.to_string())))
                .await;
        });
    }

    let resolver = Rc::new(resolver);

    let resolver_ = resolver.clone();
    let state_ = state.clone();
    let rpc = move |caller: &str, addr: &str, msg: &[u8]| {
        log::trace!("local bus: rpc call (egress): {}", addr);

        let (caller_id, remote_id, address) = match (*resolver_)(caller, addr) {
            Ok(id) => id,
            Err(err) => {
                log::debug!("rpc {} forward error: {}", addr, err);
                return async move { Ok(chunk_err(0, err).unwrap().into_bytes()) }.left_future();
            }
        };

        log::trace!(
            "local bus: rpc call (egress): {} ({} -> {})",
            address,
            caller_id,
            remote_id
        );

        let mut rx = if state_.inner.borrow().ids.contains(&remote_id) {
            let (tx, rx) = mpsc::channel(1);
            forward_local(&caller_id.to_string(), addr, msg, &state_, tx);
            rx
        } else {
            forward_net(caller_id, remote_id, address, msg, &state_)
        };

        async move {
            match rx.next().await.ok_or(Error::Cancelled) {
                Ok(chunk) => match chunk {
                    ResponseChunk::Full(vec) => Ok(vec),
                    ResponseChunk::Part(_) => {
                        Err(Error::GsbFailure("partial response".to_string()))
                    }
                },
                Err(err) => Err(err),
            }
        }
        .right_future()
    };

    let resolver_ = resolver.clone();
    let state_ = state.clone();
    let stream = move |caller: &str, addr: &str, msg: &[u8]| {
        log::trace!("local bus: stream call (egress): {}", addr);

        let (caller_id, remote_id, address) = match (*resolver_)(caller, addr) {
            Ok(id) => id,
            Err(err) => {
                log::debug!("local bus: stream call (egress) to {} error: {}", addr, err);
                return futures::stream::once(async move { chunk_err(0, err) })
                    .boxed_local()
                    .left_stream();
            }
        };

        log::trace!(
            "local bus: stream call (egress): {} ({} -> {})",
            address,
            caller_id,
            remote_id
        );

        let rx = if state_.inner.borrow().ids.contains(&remote_id) {
            let (tx, rx) = mpsc::channel(1);
            forward_local(&caller_id.to_string(), addr, msg, &state_, tx);
            rx
        } else {
            forward_net(caller_id, remote_id, address, msg, &state_)
        };
        let eos = Rc::new(AtomicBool::new(false));
        let eos_chain = eos.clone();

        rx.map(move |v| {
            v.is_full().then(|| eos.store(true, Relaxed));
            Ok(v)
        })
        .chain(StreamOnceIf::new(
            move || !eos_chain.load(Relaxed),
            move || Ok(ResponseChunk::Full(Vec::new())),
        ))
        .boxed_local()
        .right_stream()
    };

    log::debug!("local bus: subscribing to {}", address);
    local_bus::subscribe(address, rpc, stream);
}

/// Forward broadcast messages from the network to the local bus
fn spawn_broadcast_handler(rx: NetReceiver) {
    tokio::task::spawn_local(StreamExt::for_each(rx, move |payload| {
        async move {
            let client = CLIENT
                .with(|c| c.borrow().clone())
                .ok_or_else(|| anyhow::anyhow!("network not initialized"))?;
            let session = client.server_session().await?;
            session
                .broadcast(payload, DEFAULT_BROADCAST_NODE_COUNT)
                .await
                .context("broadcast failed")
        }
        .then(|result: anyhow::Result<()>| async move {
            if let Err(e) = result {
                log::debug!("unable to broadcast message: {}", e)
            }
        })
    }));
}

/// Forward GSB from the network to the local bus
fn spawn_inbound_handler(
    rx: impl Stream<Item = Vec<u8>> + 'static,
    remote_id: NodeId,
    reliable: bool,
    state: State,
) {
    tokio::task::spawn_local(StreamExt::for_each(rx, move |payload| {
        let state = state.clone();
        log::trace!("local bus handler -> inbound message");

        async move {
            match decode_message(payload.as_slice()) {
                Ok(Some(GsbMessage::CallRequest(request @ ya_sb_proto::CallRequest { .. }))) => {
                    handle_request(request, remote_id, state, reliable)
                }
                Ok(Some(GsbMessage::CallReply(reply @ ya_sb_proto::CallReply { .. }))) => {
                    handle_reply(reply, remote_id, state)
                }
                Ok(Some(GsbMessage::BroadcastRequest(
                    request @ ya_sb_proto::BroadcastRequest { .. },
                ))) => handle_broadcast(request, remote_id),
                Ok(None) => {
                    log::trace!("received a partial message");
                    Ok(())
                }
                Err(err) => anyhow::bail!("received message error: {}", err),
                _ => anyhow::bail!("unexpected message type"),
            }
        }
        .then(|result| async move {
            if let Err(e) = result {
                log::debug!("ingress message error: {}", e)
            }
        })
    }));
}

/// Forward messages from the network to the local bus
fn handle_request(
    request: ya_sb_proto::CallRequest,
    remote_id: NodeId,
    state: State,
    reliable: bool,
) -> anyhow::Result<()> {
    let caller_id = NodeId::from_str(&request.caller).ok();
    if !caller_id.map(|id| id == remote_id).unwrap_or(false) {
        anyhow::bail!("invalid caller id: {}", request.caller);
    }

    let address = request.address;
    let caller_id = caller_id.unwrap();
    let request_id = request.request_id;
    let request_id_map = request_id.clone();
    let request_id_map2 = request_id.clone();

    log::trace!(
        "handle request {} to {} from {}",
        request_id,
        address,
        remote_id
    );

    let eos = Rc::new(AtomicBool::new(false));
    let eos_map = eos.clone();
    let eos_chain = eos.clone();

    let stream = match {
        let inner = state.inner.borrow();
        inner
            .services
            .iter()
            .find(|&id| address.starts_with(id))
            // replaces  /net/<dest_node_id>/test/1 --> /public/test/1
            .map(|s| address.replacen(s, net::PUBLIC_PREFIX, 1))
    } {
        Some(address) => {
            log::trace!("handle request: calling: {}", address);
            local_bus::call_stream(&address, &request.caller, &request.data).left_stream()
        }
        None => {
            log::trace!("handle request failed: unknown address: {}", address);
            let err = Error::GsbBadRequest(format!("unknown address: {}", address));
            futures::stream::once(futures::future::err(err)).right_stream()
        }
    }
    .map(move |result| match result {
        Ok(chunk) => {
            chunk.is_full().then(|| eos_map.store(true, Relaxed));
            reply_ok(request_id.clone(), chunk)
        }
        Err(err) => {
            eos_map.store(true, Relaxed);
            reply_err(request_id.clone(), err)
        }
    })
    .chain(StreamOnceIf::new(
        move || !eos_chain.load(Relaxed),
        move || reply_eos(request_id_map.clone()),
    ))
    .filter_map(move |reply| {
        let request_id = request_id_map2.clone();
        async move {
            match encode_message(reply) {
                Ok(vec) => {
                    log::trace!(
                        "handle request {}: reply chunk ({} B)",
                        request_id,
                        vec.len()
                    );
                    Some(Ok::<Vec<u8>, mpsc::SendError>(vec))
                }
                Err(e) => {
                    log::debug!("handle request: reply encoding error: {}", e);
                    None
                }
            }
        }
    });

    tokio::task::spawn_local(
        async move {
            let sink = state.forward_sink(caller_id, reliable).await?;
            stream.forward(sink).await?;
            Ok::<_, anyhow::Error>(())
        }
        .then(|result| async move {
            if let Err(e) = result {
                log::debug!("reply forward error: {}", e)
            }
        }),
    );

    Ok(())
}

/// Forward replies from the network to the local bus
fn handle_reply(
    reply: ya_sb_proto::CallReply,
    remote_id: NodeId,
    state: State,
) -> anyhow::Result<()> {
    let full = reply.reply_type == ya_sb_proto::CallReplyType::Full as i32;

    log::trace!(
        "handle reply from node {} (full: {}, code: {}, id: {}) {} B",
        remote_id,
        full,
        reply.code,
        reply.request_id,
        reply.data.len(),
    );

    let mut request = match {
        let inner = state.inner.borrow();
        inner.requests.get(&reply.request_id).cloned()
    } {
        Some(request) => {
            if request.remote_id == remote_id {
                if full {
                    let mut inner = state.inner.borrow_mut();
                    inner.requests.remove(&reply.request_id);
                }
                request
            } else {
                anyhow::bail!("invalid reply caller for request id: {}", reply.request_id);
            }
        }
        None => anyhow::bail!("invalid reply request id: {}", reply.request_id),
    };

    let data = if reply.code == CallReplyCode::CallReplyOk as i32 {
        reply.data
    } else {
        let err = anyhow::anyhow!(
            "request {} failed with code {}",
            reply.request_id,
            reply.code
        );
        log::debug!("{}", err);
        match encode_bad_request(reply.request_id, err) {
            Ok(vec) => vec,
            Err(err) => anyhow::bail!("unable to encode error reply: {}", err),
        }
    };

    tokio::task::spawn_local(async move {
        if let Err(_) = request
            .tx
            .send(if full {
                ResponseChunk::Full(data)
            } else {
                ResponseChunk::Part(data)
            })
            .await
        {
            log::debug!("failed to forward reply: channel closed");
        }
    });

    Ok(())
}

/// Forward broadcasts from the network to the local bus
fn handle_broadcast(
    request: ya_sb_proto::BroadcastRequest,
    remote_id: NodeId,
) -> anyhow::Result<()> {
    let caller_id = NodeId::from_str(&request.caller).ok();
    if !caller_id.map(|id| id == remote_id).unwrap_or(false) {
        anyhow::bail!("invalid broadcast caller id: {}", request.caller);
    }

    log::trace!(
        "Received broadcast to topic {} from [{}].",
        &request.topic,
        &request.caller
    );

    let caller = caller_id.unwrap().to_string();

    tokio::task::spawn_local(async move {
        let data: Rc<[u8]> = request.data.into();
        let topic = request.topic;

        let handlers = BCAST_HANDLERS.lock().unwrap();
        for handler in BCAST
            .resolve(&topic)
            .into_iter()
            .filter_map(|e| handlers.get(e.as_ref()).clone())
        {
            let mut h = handler.lock().unwrap();
            (*(h))(caller.clone(), data.as_ref());
        }
    });

    Ok(())
}

#[derive(Clone)]
struct State {
    inner: Rc<RefCell<StateInner>>,
}

#[derive(Default)]
struct StateInner {
    requests: HashMap<String, Request<BusSender>>,
    routes: HashMap<NetSinkKey, NetSender>,
    forward: HashMap<NetSinkKey, NetSinkKind>,
    ids: HashSet<NodeId>,
    services: HashSet<String>,
}

impl State {
    fn new(ids: impl IntoIterator<Item = NodeId>, services: HashSet<String>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(StateInner {
                ids: ids.into_iter().collect(),
                services,
                ..Default::default()
            })),
        }
    }

    async fn forward_sink(&self, remote_id: NodeId, reliable: bool) -> anyhow::Result<NetSinkKind> {
        match {
            let inner = self.inner.borrow();
            inner.forward.get(&(remote_id, reliable)).cloned()
        } {
            Some(sink) => Ok(sink),
            None => {
                let client = CLIENT
                    .with(|c| c.borrow().clone())
                    .ok_or_else(|| anyhow::anyhow!("network not started"))?;

                let session = client.server_session().await?;
                let forward: NetSinkKind = if reliable {
                    PrefixedSink::new(session.forward(remote_id).await?).into()
                } else {
                    session.forward_unreliable(remote_id).await?.into()
                };

                let mut inner = self.inner.borrow_mut();
                inner.forward.insert((remote_id, reliable), forward.clone());

                Ok(forward)
            }
        }
    }
}

#[derive(Clone)]
struct Request<S: Clone> {
    caller_id: NodeId,
    remote_id: NodeId,
    address: String,
    tx: S,
}

struct StreamOnceIf<P, V, T>
where
    P: Fn() -> bool,
    V: Fn() -> T,
{
    predicate: P,
    value: V,
    done: bool,
}

impl<P, V, T> StreamOnceIf<P, V, T>
where
    P: Fn() -> bool,
    V: Fn() -> T,
{
    fn new(predicate: P, value: V) -> Self {
        Self {
            predicate,
            value,
            done: false,
        }
    }
}

impl<P, V, T> Stream for StreamOnceIf<P, V, T>
where
    P: Fn() -> bool + Unpin,
    V: Fn() -> T + Unpin,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if (self.as_ref().predicate)() {
            if self.done {
                Poll::Ready(None)
            } else {
                let this = self.get_mut();
                this.done = true;
                let value = (this.value)();
                Poll::Ready(Some(value))
            }
        } else {
            Poll::Ready(None)
        }
    }
}

fn encode_request(
    caller: NodeId,
    address: String,
    request_id: String,
    data: Vec<u8>,
) -> anyhow::Result<Vec<u8>> {
    let message = GsbMessage::CallRequest(ya_sb_proto::CallRequest {
        caller: caller.to_string(),
        address,
        request_id,
        data,
    });
    Ok(encode_message(message)?)
}

fn encode_bad_request(request_id: impl ToString, error: impl ToString) -> anyhow::Result<Vec<u8>> {
    encode_error(request_id, error, CallReplyCode::CallReplyBadRequest as i32)
}

fn encode_error(
    request_id: impl ToString,
    error: impl ToString,
    code: i32,
) -> anyhow::Result<Vec<u8>> {
    let message = GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id: request_id.to_string(),
        code,
        reply_type: ya_sb_proto::CallReplyType::Full as i32,
        data: error.to_string().into_bytes(),
    });
    Ok(encode_message(message)?)
}

pub(crate) fn encode_message(msg: GsbMessage) -> Result<Vec<u8>, Error> {
    use prost::Message;

    let packet = ya_sb_proto::Packet { packet: Some(msg) };
    let len: usize = packet.encoded_len();

    let mut dst = Vec::with_capacity(4 + len);
    dst.extend((len as u32).to_be_bytes());
    packet
        .encode(&mut dst)
        .map_err(|e| Error::EncodingProblem(e.to_string()))?;

    Ok(dst)
}

pub(crate) fn decode_message(src: &[u8]) -> Result<Option<GsbMessage>, Error> {
    use prost::Message;

    let msg_length = if src.len() < 4 {
        return Ok(None);
    } else {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&src[0..4]);
        u32::from_be_bytes(buf) as usize
    };

    if src.len() < 4 + msg_length {
        return Ok(None);
    }

    let packet = ya_sb_proto::Packet::decode(&src[4..4 + msg_length])
        .map_err(|e| Error::EncodingProblem(e.to_string()))?;
    match packet.packet {
        Some(msg) => Ok(Some(msg)),
        None => Err(Error::EncodingProblem(
            ProtocolError::UnrecognizedMessageType.to_string(),
        )),
    }
}

fn reply_ok(request_id: impl ToString, chunk: ResponseChunk) -> GsbMessage {
    let reply_type = if chunk.is_full() {
        ya_sb_proto::CallReplyType::Full as i32
    } else {
        ya_sb_proto::CallReplyType::Partial as i32
    };

    GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id: request_id.to_string(),
        code: ya_sb_proto::CallReplyCode::CallReplyOk as i32,
        reply_type,
        data: chunk.into_bytes(),
    })
}

fn reply_err(request_id: impl ToString, err: impl ToString) -> GsbMessage {
    GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id: request_id.to_string(),
        code: ya_sb_proto::CallReplyCode::CallReplyBadRequest as i32,
        reply_type: ya_sb_proto::CallReplyType::Full as i32,
        data: err.to_string().into_bytes(),
    })
}

fn reply_eos(request_id: impl ToString) -> GsbMessage {
    GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id: request_id.to_string(),
        code: ya_sb_proto::CallReplyCode::CallReplyOk as i32,
        reply_type: ya_sb_proto::CallReplyType::Full as i32,
        data: vec![],
    })
}

fn chunk_err(request_id: impl ToString, err: impl ToString) -> Result<ResponseChunk, Error> {
    Ok(ResponseChunk::Full(encode_message(reply_err(
        request_id.to_string(),
        err,
    ))?))
}

fn parse_net_to_addr(addr: &str) -> anyhow::Result<(NodeId, String)> {
    let mut it = addr.split("/").fuse();
    if let (Some(""), Some("net"), Some(to_node_id)) = (it.next(), it.next(), it.next()) {
        let to_id = to_node_id.parse::<NodeId>()?;

        let prefix = 6 + to_node_id.len();
        let service_id = &addr[prefix..];

        if let Some(_) = it.next() {
            return Ok((to_id, net_service(format!("{}/{}", to_node_id, service_id))));
        }
    }
    anyhow::bail!("invalid net-to destination: {}", addr)
}

fn parse_from_to_addr(addr: &str) -> anyhow::Result<(NodeId, NodeId, String)> {
    let mut it = addr.split("/").fuse();
    if let (Some(""), Some("from"), Some(from_node_id), Some("to"), Some(to_node_id)) =
        (it.next(), it.next(), it.next(), it.next(), it.next())
    {
        let from_id = from_node_id.parse::<NodeId>()?;
        let to_id = to_node_id.parse::<NodeId>()?;

        let prefix = 10 + from_node_id.len();
        let service_id = &addr[prefix..];

        if let Some(_) = it.next() {
            return Ok((from_id, to_id, net_service(service_id)));
        }
    }
    anyhow::bail!("invalid net-from-to destination: {}", addr)
}

fn gen_id() -> u64 {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    rng.gen::<u64>() & 0x1f_ff_ff__ff_ff_ff_ffu64
}

#[cfg(test)]
mod tests {
    use crate::hybrid::service::{decode_message, encode_message};
    use std::iter::FromIterator;

    #[test]
    fn encode_message_compat() {
        use tokio_util::codec::Encoder;
        use ya_sb_proto::codec::GsbMessage;

        let msg = GsbMessage::CallReply(ya_sb_proto::CallReply {
            request_id: "10203040".to_string(),
            code: ya_sb_proto::CallReplyCode::CallReplyBadRequest as i32,
            reply_type: ya_sb_proto::CallReplyType::Full as i32,
            data: "err".to_string().into_bytes(),
        });
        let encoded = encode_message(msg.clone()).unwrap();

        let mut buf = bytes::BytesMut::with_capacity(msg.encoded_len());
        ya_sb_proto::codec::GsbMessageEncoder::default()
            .encode(msg.clone(), &mut buf)
            .unwrap();
        let encoded_orig = Vec::from_iter(buf.into_iter());

        assert_eq!(encoded_orig, encoded);
        assert_eq!(decode_message(encoded.as_slice()).unwrap().unwrap(), msg);
    }
}
