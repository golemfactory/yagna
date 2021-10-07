use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;
use std::net::{SocketAddr, ToSocketAddrs};
use std::pin::Pin;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use anyhow::Context as AnyhowContext;
use futures::channel::mpsc;
use futures::stream::LocalBoxStream;
use futures::{FutureExt, SinkExt, Stream, StreamExt, TryStreamExt};
use tokio::time::{self, Duration};
use url::Url;

use ya_core_model::net::{self, net_service};
use ya_core_model::NodeId;
use ya_net_server::testing::{Client, ClientBuilder};
use ya_relay_proto::codec::forward::{PrefixedSink, PrefixedStream, SinkKind};
use ya_sb_proto::codec::GsbMessage;
use ya_service_bus::{untyped as local_bus, Error, ResponseChunk};
use ya_utils_networking::resolver;

use crate::bcast::BCastService;
use crate::hybrid::crypto::IdentityCryptoProvider;

const NET_RELAY_HOST_ENV_VAR: &str = "NET_RELAY_HOST";
const DEFAULT_NET_RELAY_HOST: &str = "127.0.0.1:7464";
const DEFAULT_BROADCAST_NODE_COUNT: u32 = 12;
const DEFAULT_PING_INTERVAL: Duration = Duration::from_millis(15000);
const REQUEST_ID: AtomicUsize = AtomicUsize::new(0);

pub type BCastHandler = Box<dyn FnMut(String, &[u8]) + Send>;

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
    fn add_request(
        caller_id: NodeId,
        remote_id: NodeId,
        address: impl ToString,
        state: &State,
    ) -> (NetReceiver, String) {
        log::trace!("local bus handler -> add request to remote");

        let (tx, rx) = mpsc::channel(1);
        let request_id = REQUEST_ID.fetch_add(1, Relaxed).to_string();
        let request = Request {
            caller_id,
            remote_id,
            address: address.to_string(),
            tx,
        };

        let mut inner = state.inner.borrow_mut();
        inner.requests.insert(request_id.clone(), request);

        (rx, request_id)
    }

    fn forward_net(
        caller_id: NodeId,
        remote_id: NodeId,
        address: impl ToString,
        request_id: String,
        data: &[u8],
        state: &State,
    ) {
        let address = address.to_string();
        let state = state.clone();
        let data = data.to_vec();

        tokio::task::spawn_local(async move {
            log::trace!("local bus handler -> send message to remote");

            match state.forward_sink(remote_id, true).await {
                Ok(mut session) => {
                    match encode_request(caller_id, address, request_id, data) {
                        Ok(data) => {
                            let _ = session
                                .send(data)
                                .await
                                .map_err(|_| log::debug!("error sending message: session closed"));
                        }
                        Err(error) => log::debug!("error encoding message: {}", error),
                    };
                }
                Err(error) => log::debug!("error forwarding message: {}", error),
            };
        });
    }

    fn forward_local(caller: &str, addr: &str, data: &[u8], state: &State, tx: NetSender) {
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
                log::debug!("unknown address: {}", addr);
                return;
            }
        };

        let send = local_bus::call_stream(address.as_str(), caller, data);
        tokio::task::spawn_local(async move {
            let _ = send
                .map(|r| r.map(|c| c.into_bytes()))
                .map_err(|e| Error::GsbFailure(e.to_string()))
                .forward(tx.sink_map_err(|e| Error::GsbFailure(e.to_string())))
                .await;
        });
    }

    fn err_future<T, M>(message: M) -> futures::future::Ready<Result<T, Error>>
    where
        T: 'static,
        M: ToString,
    {
        let err = Error::GsbBadRequest(message.to_string());
        futures::future::err(err)
    }

    fn err_stream<'a, T, M>(message: M) -> LocalBoxStream<'a, Result<T, Error>>
    where
        T: 'static,
        M: ToString,
    {
        let err = Error::GsbBadRequest(message.to_string());
        futures::stream::once(async move { Err(err) }).boxed_local()
    }

    let resolver = Rc::new(resolver);

    let resolver_ = resolver.clone();
    let state_ = state.clone();
    let rpc = move |caller: &str, addr: &str, msg: &[u8]| {
        log::trace!("handle rpc {}", addr);

        let (caller_id, remote_id, address) = match (*resolver_)(caller, addr) {
            Ok(id) => id,
            Err(error) => return err_future(error).left_future(),
        };

        log::trace!(
            "sending rpc message to {} ({} -> {})",
            address,
            caller_id,
            remote_id
        );

        let mut rx = if caller_id == remote_id {
            let (tx, rx) = mpsc::channel(1);
            forward_local(&caller_id.to_string(), addr, msg, &state_, tx);
            rx
        } else {
            let (rx, request_id) = add_request(caller_id, remote_id, &address, &state_);
            forward_net(caller_id, remote_id, address, request_id, msg, &state_);
            rx
        };

        async move { rx.next().await.ok_or(Error::Cancelled) }.right_future()
    };

    let resolver_ = resolver.clone();
    let state_ = state.clone();
    let stream = move |caller: &str, addr: &str, msg: &[u8]| {
        let (caller_id, remote_id, address) = match (*resolver_)(caller, addr) {
            Ok(id) => id,
            Err(error) => return err_stream(error).left_stream(),
        };

        log::trace!(
            "sending stream message to {} ({} -> {})",
            address,
            caller_id,
            remote_id
        );

        let rx = if caller_id == remote_id {
            let (tx, rx) = mpsc::channel(1);
            forward_local(&caller_id.to_string(), addr, msg, &state_, tx);
            rx
        } else {
            let (rx, request_id) = add_request(caller_id, remote_id, &address, &state_);
            forward_net(caller_id, remote_id, address, request_id, msg, &state_);
            rx
        };

        let eos = Rc::new(AtomicBool::new(false));
        let eos_chain = eos.clone();

        rx.map(move |v| {
            // FIXME: streaming response
            eos.store(true, Relaxed);
            Ok(ResponseChunk::Full(v))
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
                    handle_request(request, remote_id, state, reliable)?;
                }
                Ok(Some(GsbMessage::CallReply(reply @ ya_sb_proto::CallReply { .. }))) => {
                    handle_reply(reply, remote_id, state)?;
                }
                Ok(Some(GsbMessage::BroadcastRequest(
                    request @ ya_sb_proto::BroadcastRequest { .. },
                ))) => {
                    handle_broadcast(request, remote_id)?;
                }
                Ok(None) => anyhow::bail!("received partial message"),
                Err(err) => anyhow::bail!("received message error: {}", err),
                _ => anyhow::bail!("unexpected message type"),
            };
            Ok(())
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

    log::trace!("sending request to {}", remote_id);

    let address = request.address;
    let caller_id = caller_id.unwrap();
    let request_id = request.request_id;
    let request_id_map = request_id.clone();

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
            local_bus::call_stream(&address, &request.caller, &request.data).left_stream()
        }
        None => {
            let err = Error::GsbBadRequest(format!("unknown address: {}", address));
            futures::stream::once(futures::future::err(err)).right_stream()
        }
    }
    .map(move |result| match result {
        Ok(chunk) => {
            chunk.is_full().then(|| eos_map.store(true, Relaxed));
            chunk_ok(request_id.clone(), chunk)
        }
        Err(err) => {
            eos_map.store(true, Relaxed);
            chunk_err(request_id.clone(), err)
        }
    })
    .chain(StreamOnceIf::new(
        move || !eos_chain.load(Relaxed),
        move || chunk_eos(request_id_map.clone()),
    ))
    .filter_map(|reply| async move {
        match encode_message(reply) {
            Ok(vec) => Some(Ok::<Vec<u8>, mpsc::SendError>(vec)),
            Err(e) => {
                log::debug!("packet encoding error: {}", e);
                None
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
    let mut request = match {
        let inner = state.inner.borrow();
        inner.requests.get(&reply.request_id).cloned()
    } {
        Some(request) => {
            if request.remote_id == remote_id {
                if reply.reply_type == ya_sb_proto::CallReplyType::Full as i32 {
                    let mut inner = state.inner.borrow_mut();
                    inner.requests.remove(&reply.request_id);
                }
                request
            } else {
                anyhow::bail!("invalid reply caller for request id: {}", reply.request_id);
            }
        }
        None => anyhow::bail!("unknown request id: {}", reply.request_id),
    };

    log::trace!("handle reply from node {}", remote_id);

    let data = if reply.code == ya_sb_proto::CallReplyCode::CallReplyOk as i32 {
        reply.data
    } else {
        let err = anyhow::anyhow!("request failed with code {}", reply.code);
        match encode_bad_request(reply.request_id, err) {
            Ok(vec) => vec,
            Err(err) => anyhow::bail!("unable to encode error reply: {}", err),
        }
    };

    tokio::task::spawn_local(async move {
        log::trace!("forward reply data");
        let _ = request.tx.send(data).await;
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

    let topic = request.topic;
    let data: Rc<[u8]> = request.data.into();
    let caller = caller_id.unwrap().to_string();

    let handlers = BCAST_HANDLERS.lock().unwrap();
    for handler in BCAST
        .resolve(&topic)
        .into_iter()
        .filter_map(|e| handlers.get(e.as_ref()).clone())
    {
        let mut h = handler.lock().unwrap();
        (*(h))(caller.clone(), data.as_ref());
    }

    Ok(())
}

#[derive(Clone)]
struct State {
    inner: Rc<RefCell<StateInner>>,
}

#[derive(Default)]
struct StateInner {
    requests: HashMap<String, Request<NetSender>>,
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

fn encode_bad_request(request_id: String, error: impl ToString) -> anyhow::Result<Vec<u8>> {
    encode_error(
        request_id,
        error,
        ya_sb_proto::CallReplyCode::CallReplyBadRequest as i32,
    )
}

fn encode_error(request_id: String, error: impl ToString, code: i32) -> anyhow::Result<Vec<u8>> {
    let message = GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id,
        code,
        reply_type: ya_sb_proto::CallReplyType::Full as i32,
        data: error.to_string().into_bytes(),
    });
    Ok(encode_message(message)?)
}

pub(crate) fn encode_message(msg: GsbMessage) -> Result<Vec<u8>, Error> {
    use tokio_util::codec::Encoder;

    let mut buf = bytes::BytesMut::with_capacity(msg.encoded_len());
    ya_sb_proto::codec::GsbMessageEncoder::default()
        .encode(msg, &mut buf)
        .map_err(|e| Error::EncodingProblem(e.to_string()))?;
    Ok(Vec::from_iter(buf.into_iter()))
}

pub(crate) fn decode_message(src: &[u8]) -> Result<Option<GsbMessage>, Error> {
    use tokio_util::codec::Decoder;

    let mut buf = bytes::BytesMut::from_iter(src.iter().cloned());
    Ok(ya_sb_proto::codec::GsbMessageDecoder::default()
        .decode(&mut buf)
        .map_err(|e| Error::EncodingProblem(e.to_string()))?)
}

fn chunk_ok(request_id: String, chunk: ResponseChunk) -> GsbMessage {
    let reply_type = if chunk.is_full() {
        ya_sb_proto::CallReplyType::Full as i32
    } else {
        ya_sb_proto::CallReplyType::Partial as i32
    };

    GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id,
        code: ya_sb_proto::CallReplyCode::CallReplyOk as i32,
        reply_type,
        data: chunk.into_bytes(),
    })
}

fn chunk_err(request_id: String, err: ya_service_bus::Error) -> GsbMessage {
    GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id,
        code: ya_sb_proto::CallReplyCode::ServiceFailure as i32,
        reply_type: ya_sb_proto::CallReplyType::Full as i32,
        data: format!("{}", err).into_bytes(),
    })
}

fn chunk_eos(request_id: String) -> GsbMessage {
    GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id,
        code: ya_sb_proto::CallReplyCode::CallReplyOk as i32,
        reply_type: ya_sb_proto::CallReplyType::Full as i32,
        data: vec![],
    })
}

fn parse_net_to_addr(addr: &str) -> anyhow::Result<(NodeId, String)> {
    let mut it = addr.split("/").fuse();
    if let (Some(""), Some("net"), Some(to_node_id)) = (it.next(), it.next(), it.next()) {
        let to_id = to_node_id.parse::<NodeId>()?;

        let prefix = 6 + to_node_id.len();
        let service_id = &addr[prefix..];

        if let Some(_) = it.next() {
            return Ok((to_id, net_service(service_id)));
        }
    }
    anyhow::bail!("invalid net-from destination: {}", addr)
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
    anyhow::bail!("invalid net-from destination: {}", addr)
}
