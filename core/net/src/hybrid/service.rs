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

use bytes::BytesMut;
use futures::channel::mpsc;
use futures::stream::LocalBoxStream;
use futures::{FutureExt, SinkExt, Stream, StreamExt, TryStreamExt};
use tokio::time::{self, Duration};
use tokio_util::codec::{Decoder, Encoder};
use url::Url;

use ya_core_model::net::{self, net_service};
use ya_core_model::NodeId;
use ya_net_server::testing::{Client, ClientBuilder};
use ya_relay_proto::codec::forward::{PrefixedSink, PrefixedStream, SinkKind};
use ya_sb_proto::codec::{GsbMessage, GsbMessageDecoder, GsbMessageEncoder};
use ya_service_bus::{untyped as local_bus, Error, ResponseChunk};
use ya_utils_networking::resolver;

use crate::bcast::BCastService;
use crate::hybrid::crypto::IdentityCryptoProvider;
use crate::service::parse_from_addr;

const NET_RELAY_HOST_ENV_VAR: &str = "NET_RELAY_HOST";
const DEFAULT_NET_RELAY_HOST: &str = "127.0.0.1:7464";
const PING_INTERVAL: Duration = Duration::from_millis(15000);
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

    log::info!("starting hybrid network client with id: {}", default_id);

    let client = ClientBuilder::from_url(url)
        .crypto(provider)
        .connect()
        .build()
        .await?;
    let (btx, brx) = mpsc::channel(1);

    CLIENT.with(|c| {
        c.borrow_mut().replace(client.clone());
    });
    BCAST_SENDER.lock().unwrap().replace(btx);

    let receiver = client.forward_receiver().await.unwrap();
    let services = ids.iter().map(|id| net_service(id)).collect();
    let state = State::new(ids, services);

    // outbound traffic
    spawn_broadcast_handler(brx);
    spawn_local_bus_handler(net::BUS_ID, state.clone(), move |_| Ok(default_id.clone()));
    spawn_local_bus_handler("/from", state.clone(), {
        let state = state.clone();
        move |addr| {
            let (from_node, _) = match parse_from_addr(addr) {
                Ok(v) => v,
                Err(e) => anyhow::bail!("invalid address: {}", e),
            };
            if !state.inner.borrow().ids.contains(&from_node) {
                anyhow::bail!("unknown identity: {:?}", from_node);
            }
            Ok(from_node)
        }
    });

    // inbound traffic
    tokio::task::spawn_local(receiver.for_each(move |fwd| {
        let state = state.clone();
        async move {
            let key = (fwd.node_id, fwd.reliable);
            let mut tx = match {
                let inner = state.inner.borrow();
                inner.routes.get(&key).cloned()
            } {
                Some(tx) => tx,
                None => {
                    let (tx, rx) = mpsc::channel(1);
                    {
                        let mut inner = state.inner.borrow_mut();
                        inner.routes.insert(key, tx.clone());
                    }

                    let rx = if fwd.reliable {
                        PrefixedStream::new(rx)
                            .inspect_err(|e| log::debug!("stream error: {}", e))
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

    // Keep server connection alive by pinging every `PING_INTERVAL` seconds.
    let ping_client = client.clone();
    tokio::task::spawn_local(async move {
        let mut interval = time::interval(PING_INTERVAL);
        loop {
            interval.tick().await;
            if let Ok(session) = ping_client.server_session().await {
                log::trace!("Sending ping to keep session alive.");
                let _ = session.ping().await;
            }
        }
    });

    Ok(())
}

fn spawn_local_bus_handler<F>(address: &'static str, state: State, id_fn: F)
where
    F: Fn(&str) -> anyhow::Result<NodeId> + 'static,
{
    fn add_request(caller: NodeId, address: String, state: &State) -> (NetReceiver, String) {
        let (tx, rx) = mpsc::channel(1);
        let request_id = REQUEST_ID.fetch_add(1, Relaxed).to_string();
        let request = Request {
            caller,
            address,
            tx,
        };

        let mut inner = state.inner.borrow_mut();
        inner.requests.insert(request_id.clone(), request);

        (rx, request_id)
    }

    fn send_message(caller: NodeId, request_id: String, data: &[u8], state: &State) {
        let state = state.clone();
        let data = data.to_vec();

        tokio::task::spawn_local(async move {
            match state.forward_sink(caller, true).await {
                Ok(mut session) => {
                    let data = match chunk_encode_full(request_id, data) {
                        Ok(data) => data,
                        Err(error) => {
                            log::debug!("error encoding message: {}", error);
                            return;
                        }
                    };
                    if let Err(_) = session.send(data).await {
                        log::debug!("error sending message: session closed");
                    }
                }
                Err(error) => log::debug!("error forwarding message: {}", error),
            };
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

    let id_fn = Rc::new(id_fn);

    let state_ = state.clone();
    let id_fn_ = id_fn.clone();
    let rpc = move |_caller: &str, address: &str, msg: &[u8]| {
        let caller = match (*id_fn_)(address) {
            Ok(caller) => caller,
            Err(error) => return err_future(error).left_future(),
        };

        log::trace!("sending rpc message to {} (caller: {})", address, caller);

        let (mut rx, request_id) = add_request(caller, address.to_string(), &state_);
        send_message(caller, request_id, msg, &state_);

        async move { rx.next().await.ok_or(Error::Cancelled) }.right_future()
    };

    let state_ = state.clone();
    let id_fn_ = id_fn.clone();
    let stream = move |_caller: &str, address: &str, msg: &[u8]| {
        let caller = match (*id_fn_)(address) {
            Ok(caller) => caller,
            Err(error) => return err_stream(error).left_stream(),
        };

        log::trace!("sending stream message to {} (caller: {})", address, caller);

        let (rx, request_id) = add_request(caller, address.to_string(), &state_);
        send_message(caller, request_id, msg, &state_);

        rx.map(|v| Ok(ResponseChunk::Part(v)))
            .boxed_local()
            .right_stream()
    };

    local_bus::subscribe(address, rpc, stream);
}

fn spawn_broadcast_handler(rx: NetReceiver) {
    tokio::task::spawn_local(StreamExt::for_each(rx, move |payload| {
        async move {
            let client = CLIENT
                .with(|c| c.borrow().clone())
                .ok_or_else(|| anyhow::anyhow!("network not initialized"))?;
            let session = client.server_session().await?;
            session.broadcast(payload, 12).await?;
            Ok(())
        }
        .then(|result: anyhow::Result<()>| async move {
            if let Err(e) = result {
                log::debug!("unable to broadcast message: {}", e)
            }
        })
    }));
}

/// Forward messages from the network to the local bus
fn spawn_inbound_handler(
    rx: impl Stream<Item = Vec<u8>> + 'static,
    remote_id: NodeId,
    reliable: bool,
    state: State,
) {
    tokio::task::spawn_local(StreamExt::for_each(rx, move |payload| {
        let state = state.clone();

        async move {
            let mut bytes = BytesMut::from_iter(payload.into_iter());

            match GsbMessageDecoder::new().decode(&mut bytes)? {
                Some(GsbMessage::CallRequest(request @ ya_sb_proto::CallRequest { .. })) => {
                    handle_request(request, remote_id, state, reliable)?;
                }
                Some(GsbMessage::CallReply(reply @ ya_sb_proto::CallReply { .. })) => {
                    handle_reply(reply, remote_id, state)?;
                }
                Some(GsbMessage::BroadcastRequest(
                    request @ ya_sb_proto::BroadcastRequest { .. },
                )) => {
                    handle_broadcast(request, remote_id)?;
                }
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

    log::trace!("request {:?}", request);

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
            if chunk.is_full() {
                eos_map.store(true, Relaxed);
            }
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
        let mut bytes = BytesMut::with_capacity(reply.encoded_len());
        let mut encoder = GsbMessageEncoder::default();
        match encoder.encode(reply, &mut bytes) {
            Ok(_) => Some(Ok::<BytesMut, mpsc::SendError>(bytes)),
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
            if request.caller == remote_id {
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

    tokio::task::spawn_local(async move {
        let _ = request.tx.send(reply.data).await;
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
                    .ok_or_else(|| anyhow::anyhow!("network client not started"))?;
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
    caller: NodeId,
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

fn chunk_encode_full(request_id: String, data: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let message = chunk_ok(request_id.clone(), ResponseChunk::Full(data));

    let mut bytes = BytesMut::with_capacity(message.encoded_len());
    let mut encoder = GsbMessageEncoder::default();
    encoder.encode(message, &mut bytes)?;
    Ok(Vec::from_iter(bytes.into_iter()))
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

fn chunk_eos(request_id: String) -> GsbMessage {
    GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id,
        code: ya_sb_proto::CallReplyCode::CallReplyOk as i32,
        reply_type: ya_sb_proto::CallReplyType::Full as i32,
        data: vec![],
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
