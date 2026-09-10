use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::future::Future;
use std::net::{SocketAddr, ToSocketAddrs};
use std::rc::Rc;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::task::Poll;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context as AnyhowContext};
use ethsign::PublicKey;
use futures::channel::{mpsc, oneshot};
use futures::stream::LocalBoxStream;
use futures::{FutureExt, SinkExt, Stream, StreamExt, TryFutureExt, TryStreamExt};
use metrics::counter;
use tokio::sync::RwLock;
use tokio_stream::wrappers::UnboundedReceiverStream;
use url::Url;

use ya_core_model::identity::event::IdentityEvent;
use ya_core_model::net::local::{
    BindBroadcastError, BroadcastMessage, NewNeighbour, SendBroadcastMessage, SendBroadcastStub,
};
use ya_core_model::net::GenericNetError;
use ya_core_model::{identity, net, NodeId};
use ya_relay_client::channels::{ForwardReceiver, ForwardSender, PrefixedStream};
use ya_relay_client::crypto::CryptoProvider;
use ya_relay_client::model::{Payload, TransportType};
use ya_relay_client::{AuthenticatedIdentities, Client, ClientBuilder, FailFast};
use ya_sb_proto::codec::GsbMessage;
use ya_sb_proto::CallReplyCode;
use ya_sb_util::RevPrefixes;
use ya_service_bus::timeout::IntoTimeoutFuture;
use ya_service_bus::untyped::{Fn4HandlerExt, Fn4StreamHandlerExt};
use ya_service_bus::{
    serialization, typed, untyped as local_bus, Error, ResponseChunk, RpcEndpoint, RpcMessage,
};
use ya_utils_networking::resolver;

use crate::bcast::BCastService;
use crate::config::Config;
use crate::hybrid::codec;
use crate::hybrid::codec::encode_message;
use crate::hybrid::crypto::IdentityCryptoProvider;
use crate::service::NET_TYPE;
use crate::{broadcast, NetType};

type BusSender = mpsc::Sender<ResponseChunk>;
type BusReceiver = mpsc::Receiver<ResponseChunk>;
type NetSender = mpsc::Sender<Payload>;
type NetSinkKind = ForwardSender;
type NetSinkKey = (NodeId, TransportType);

#[derive(Clone)]
struct NetRoute {
    sender: NetSender,
    authenticated_identities: Rc<RefCell<Option<AuthenticatedIdentities>>>,
}

impl NetRoute {
    fn new(sender: NetSender, authenticated_identities: Option<AuthenticatedIdentities>) -> Self {
        Self {
            sender,
            authenticated_identities: Rc::new(RefCell::new(authenticated_identities)),
        }
    }

    fn update_authentication(&self, incoming: Option<AuthenticatedIdentities>) {
        let mut current = self.authenticated_identities.borrow_mut();
        if current.as_deref() != incoming.as_deref() {
            current.take();
        }
    }
}

lazy_static::lazy_static! {
    pub(crate) static ref BCAST: BCastService = Default::default();
    pub(crate) static ref SHUTDOWN_TX: Arc<RwLock<Option<oneshot::Sender<()>>>> = Default::default();
}

pub struct Net;

impl Net {
    pub async fn gsb<Context>(_: Context, config: Config) -> anyhow::Result<()> {
        ya_service_bus::serialization::CONFIG.set_compress(true);

        let (default_id, ids) = crate::service::identities().await?;
        let (started_tx, started_rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        log::info!(
            "Hybrid NET - Using default identity as network id: {:?}",
            default_id
        );

        std::thread::spawn(move || {
            let system = actix::System::new();
            system.block_on(async move {
                SHUTDOWN_TX.write().await.replace(shutdown_tx);

                let result = start_network(Arc::new(config), default_id, ids).await;
                started_tx.send(result).expect("Unable to start network");
                let _ = shutdown_rx.await;
            });
        });

        started_rx
            .await
            .map_err(|_| anyhow!("Error starting network"))?
    }

    pub async fn shutdown() -> anyhow::Result<()> {
        if typed::service(ya_core_model::net::local::BUS_ID)
            .send(ya_core_model::net::local::Shutdown {})
            .timeout(std::time::Duration::from_secs(30).into())
            .await
            .is_err()
        {
            log::info!("NET shutdown due to timeout.");
        }

        if let Some(sender) = { SHUTDOWN_TX.write().await.take() } {
            let _ = sender.send(());
        }
        Ok(())
    }
}

pub async fn start_network(
    config: Arc<Config>,
    default_id: NodeId,
    ids: Vec<NodeId>,
) -> anyhow::Result<()> {
    counter!("net.connections.p2p", 0);
    counter!("net.connections.relay", 0);

    log::info!("Starting network (hybrid) with identity: {default_id}");

    let broadcast_size = (config.broadcast_size, config.pub_broadcast_size);
    let crypto = IdentityCryptoProvider::new(default_id);
    let client = build_client(config, crypto.clone()).await?;

    super::cli::bind_service(client.clone());

    let receiver = client.clone().forward_receiver().await.unwrap();
    let mut services: HashSet<_> = Default::default();
    ids.iter().for_each(|id| {
        services.insert(net::net_service_udp(id));
        services.insert(net::net_service(id));
        services.insert(net::net_transfer_service(id));
    });
    let state = State::new(ids, services);

    // outbound traffic
    let net_handler = || {
        move |_: &str, addr: &str| match parse_net_to_addr(addr) {
            Ok((to, addr)) => Ok((default_id, to, addr)),
            Err(err) => anyhow::bail!("invalid address: {}", err),
        }
    };

    {
        let client = client.clone();
        typed::bind(
            ya_core_model::net::local::BUS_ID,
            move |_: ya_core_model::net::local::Shutdown| {
                let mut client = client.clone();
                async move {
                    client
                        .shutdown()
                        .map_err(|e| GenericNetError(e.to_string()))
                        .await
                }
            },
        );
    }

    bind_local_bus(
        client.clone(),
        net::BUS_ID_UDP,
        state.clone(),
        TransportType::Unreliable,
        net_handler(),
    );
    bind_local_bus(
        client.clone(),
        net::BUS_ID,
        state.clone(),
        TransportType::Reliable,
        net_handler(),
    );
    bind_local_bus(
        client.clone(),
        net::BUS_ID_TRANSFER,
        state.clone(),
        TransportType::Transfer,
        net_handler(),
    );

    let from_handler = || {
        let state_from = state.clone();
        move |_: &str, addr: &str| {
            let (from, to, addr) =
                parse_from_to_addr(addr).map_err(|e| anyhow::anyhow!("invalid address: {}", e))?;
            if !state_from.inner.borrow().ids.contains(&from) {
                anyhow::bail!("Trying to send message from unknown identity: {}", from);
            }
            Ok((from, to, addr))
        }
    };

    bind_local_bus(
        client.clone(),
        "/from",
        state.clone(),
        TransportType::Reliable,
        from_handler(),
    );
    bind_local_bus(
        client.clone(),
        "/udp/from",
        state.clone(),
        TransportType::Unreliable,
        from_handler(),
    );
    bind_local_bus(
        client.clone(),
        "/transfer/from",
        state.clone(),
        TransportType::Transfer,
        from_handler(),
    );

    tokio::task::spawn_local(forward_handler(client.clone(), receiver, state.clone()));

    bind_broadcast_handlers(client.clone(), broadcast_size);
    bind_identity_event_handler(client.clone(), crypto).await;
    bind_neighbourhood_bcast(client.clone()).await?;

    if let Some(address) = client.public_addr().await {
        log::info!("Public address: {}", address);
        counter!("net.public-addresses", 1);
    } else {
        counter!("net.public-addresses", 0);
    }

    Ok(())
}

async fn build_client(
    config: Arc<Config>,
    crypto: impl CryptoProvider + 'static,
) -> anyhow::Result<Client> {
    let addr = resolve_relay_addr(&config)
        .await
        .map_err(|e| anyhow!("Resolving hybrid NET relay server failed. Error: {}", e))?;
    let url = Url::parse(&format!("udp://{addr}"))?;

    ClientBuilder::from_url(url)
        .crypto(crypto)
        .listen(config.bind_url.clone())
        .expire_session_after(config.session_expiration)
        .session_request_timeout(config.session_request_timeout)
        .connect(FailFast::No)
        .build()
        .await
}

struct RetryArgs {
    max_retries: u64,
    start_retry_timeout: u64,
    add_seconds_every_retry: u64,
}
async fn resolve_srv_record_with_retries(prefix: &str, args: RetryArgs) -> anyhow::Result<String> {
    let mut retries = 0;
    let mut timeout_s = args.start_retry_timeout;
    log::info!("Resolving {prefix} SRV record...");
    loop {
        match resolver::resolve_yagna_srv_record(prefix).await {
            Ok(addr) => {
                log::info!("SRV record {prefix} resolved to: {addr}");
                break Ok(addr);
            }
            Err(err) => {
                if retries >= args.max_retries {
                    return Err(anyhow!(
                        "Failed to resolve {prefix} SRV record: {err} after {retries} retries"
                    ));
                }
                log::warn!(
                    "Failed to resolve {prefix} SRV record: {err}. Trying again in {timeout_s} seconds",
                );
                tokio::time::sleep(std::time::Duration::from_secs(timeout_s)).await;
                retries += 1;
                timeout_s += args.add_seconds_every_retry;
                log::info!("Retrying ({retries}) to resolve {prefix} SRV record...");
            }
        }
    }
}

async fn resolve_relay_addr(config: &Config) -> anyhow::Result<SocketAddr> {
    let host_port = match &config.host {
        Some(val) => val.to_string(),
        None => {
            resolve_srv_record_with_retries(
                "_net_relay_2._udp",
                RetryArgs {
                    max_retries: 5,
                    start_retry_timeout: 10,
                    add_seconds_every_retry: 5,
                },
            )
            .await?
        }
    };

    log::info!("Hybrid NET relay server configured on url: udp://{host_port}");

    let (host, port) = &host_port
        .split_once(':')
        .context("Please use host:port format")?;
    let ip = resolver::try_resolve_dns_record(host).await;
    let socket = format!("{}:{}", ip, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("Invalid relay address: {ip}:{port}"))?;
    Ok(socket)
}

fn bind_local_bus<F>(
    base_client: Client,
    address: &'static str,
    state: State,
    transport: TransportType,
    resolver: F,
) where
    F: Fn(&str, &str) -> anyhow::Result<(NodeId, NodeId, String)> + 'static,
{
    let resolver = Rc::new(resolver);
    let resolver_ = resolver.clone();
    let state_ = state.clone();

    let client = base_client.clone();
    let rpc = move |caller: &str, addr: &str, msg: &[u8], no_reply: bool| {
        let (caller_id, remote_id, address) = match (*resolver_)(caller, addr) {
            Ok(id) => id,
            Err(err) => {
                log::debug!("rpc {} forward error: {}", addr, err);
                return async move { Err(Error::GsbFailure(err.to_string())) }.left_future();
            }
        };

        log::trace!(
            "local bus: rpc call (egress): {} ({} -> {}), no_reply: {no_reply}",
            address,
            caller_id,
            remote_id,
        );

        let is_local_dest = state_.inner.borrow().ids.contains(&remote_id);

        let rx = if no_reply {
            if is_local_dest {
                push_bus_to_local(caller_id, addr, msg, &state_);
            } else {
                push_bus_to_net(
                    client.clone(),
                    caller_id,
                    remote_id,
                    address,
                    msg,
                    &state_,
                    transport,
                );
            }

            None
        } else {
            let rx = if is_local_dest {
                let (tx, rx) = mpsc::channel(1);
                forward_bus_to_local(caller_id, address.as_str(), msg, &state_, tx);
                rx
            } else {
                forward_bus_to_net(
                    client.clone(),
                    caller_id,
                    remote_id,
                    address,
                    msg,
                    &state_,
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
        let (caller_id, remote_id, address) = match (*resolver)(caller, addr) {
            Ok(id) => id,
            Err(err) => {
                log::debug!("local bus: stream call (egress) to {} error: {}", addr, err);
                return futures::stream::once(
                    async move { Err(Error::GsbFailure(err.to_string())) },
                )
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

        let is_local_dest = state.inner.borrow().ids.contains(&remote_id);
        let rx = if no_reply {
            if is_local_dest {
                push_bus_to_local(caller_id, addr, msg, &state);
            } else {
                push_bus_to_net(
                    client.clone(),
                    caller_id,
                    remote_id,
                    address,
                    msg,
                    &state,
                    transport,
                );
            }
            futures::stream::empty().left_stream()
        } else {
            let rx = if is_local_dest {
                let (tx, rx) = mpsc::channel(1);
                forward_bus_to_local(caller_id, addr, msg, &state, tx);
                rx
            } else {
                forward_bus_to_net(
                    client.clone(),
                    caller_id,
                    remote_id,
                    address,
                    msg,
                    &state,
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
async fn bind_identity_event_handler(client: Client, crypto: IdentityCryptoProvider) {
    let endpoint = format!("{}/id", net::BUS_ID);

    typed::bind(endpoint.as_str(), move |event: IdentityEvent| {
        log::debug!("Identity event received: {:?}", event);

        crypto.reset_alias_cache();
        let client = client.clone();

        async move {
            match event {
                IdentityEvent::AccountUnlocked { .. } | IdentityEvent::AccountLocked { .. } => {
                    client.reconnect_server().await
                }
            }
            Ok(())
        }
    });

    match typed::service(identity::BUS_ID)
        .send(identity::Subscribe { endpoint })
        .await
    {
        Err(e) => log::warn!("Identity event subscription failed: {}", e),
        Ok(Err(e)) => log::warn!("Identity event subscription failed: {}", e),
        Ok(_) => log::debug!("Successfully subscribed to identity events"),
    }
}

/// Forward requests from and to the local bus
fn forward_bus_to_local(caller: NodeId, addr: &str, data: &[u8], state: &State, tx: BusSender) {
    let caller = caller.to_string();
    let address = match state.get_public_service(addr) {
        Some(address) => address,
        None => {
            let err = format!("Net: unknown address: {}", addr);
            handler_reply_bad_request("unknown", err, tx);
            return;
        }
    };

    log::trace!("forwarding /net call to a local endpoint: {}", address);

    let send = local_bus::call_stream(&address, &caller, data);
    tokio::task::spawn_local(async move {
        let _ = send
            .forward(tx.sink_map_err(|e| Error::GsbFailure(e.to_string())))
            .await;
    });
}

fn push_bus_to_local(caller: NodeId, addr: &str, data: &[u8], state: &State) {
    let caller = caller.to_string();
    let address = match state.get_public_service(addr) {
        Some(address) => address,
        None => {
            log::debug!("Net: unknown address: {}", addr);
            return;
        }
    };

    log::trace!("pushing /net message to a local endpoint: {}", address);

    let send = local_bus::push(&address, &caller, data);
    tokio::task::spawn_local(async move {
        let _ = send.await;
    });
}

/// Forward requests from local bus to the network
fn forward_bus_to_net(
    client: Client,
    caller_id: NodeId,
    remote_id: NodeId,
    address: impl ToString,
    msg: &[u8],
    state: &State,
    transport: TransportType,
) -> BusReceiver {
    let address = address.to_string();
    let state = state.clone();
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

    let request = Request {
        caller_id,
        remote_id,
        address: address.clone(),
        tx: tx.clone(),
    };
    {
        let mut inner = state.inner.borrow_mut();
        inner.requests.insert(request_id.clone(), request);
    }

    tokio::task::spawn_local(async move {
        log::debug!(
            "Local bus handler ({caller_id} -> {remote_id}), address: {address}, id: {request_id} -> send message to remote ({} B)",
            msg.len()
        );

        match state.forward_sink(client, remote_id, transport).await {
            Ok(mut sink) => {
                let _ = sink.send(msg.into()).await.map_err(|_| {
                    let err = "Net: error sending message: session closed".to_string();
                    handler_reply_service_err(request_id, err, tx);
                });
            }
            Err(error) => {
                let err = format!("Net: error forwarding message: {:?}", error);
                handler_reply_service_err(request_id, err, tx);
            }
        };
    });

    rx
}

fn push_bus_to_net(
    client: Client,
    caller_id: NodeId,
    remote_id: NodeId,
    address: impl ToString,
    msg: &[u8],
    state: &State,
    transport: TransportType,
) {
    let address = address.to_string();
    let state = state.clone();
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

        match state
            .forward_sink(client.clone(), remote_id, transport)
            .await
        {
            Ok(mut sink) => {
                let _ = sink.send(msg.into()).await.map_err(|_| {
                    log::debug!("Net: error sending message: session closed");
                });
            }
            Err(error) => {
                log::debug!("Net: error forwarding message: {}", error);
            }
        };
    });
}

/// Forward broadcast messages from the local bus to the network
fn broadcast_handler(
    client: Client,
    caller: &str,
    _addr: &str,
    msg: &[u8],
    broadcast_size: (u32, u32),
) -> impl Future<Output = Result<Vec<u8>, Error>> {
    let message = msg.to_vec();
    let caller = caller.to_string();

    async move {
        let stub: SendBroadcastStub = serialization::from_slice(&message)
            .map_err(|e| Error::GsbFailure(format!("Invalid broadcast message: {e}")))?;

        let request = GsbMessage::BroadcastRequest(ya_sb_proto::BroadcastRequest {
            //data: serialization::to_vec(&message)?,
            data: message,
            caller,
            topic: stub.topic,
        });

        let payload = encode_message(request).map_err(|e| Error::EncodingProblem(e.to_string()))?;

        let broadcast_size = if client.public_addr().await.is_some() {
            broadcast_size.1
        } else {
            broadcast_size.0
        };

        client
            .broadcast(payload, broadcast_size)
            .await
            .map_err(|e| Error::GsbFailure(format!("Broadcast failed: {e}")))?;

        Ok(serialization::to_vec(&Ok::<(), ()>(())).unwrap())
    }
    .then(|result| async move {
        if let Err(e) = &result {
            log::debug!("Unable to broadcast message: {e}")
        }
        result
    })
}

fn bind_broadcast_handlers(client: Client, broadcast_size: (u32, u32)) {
    let _ = typed::bind(
        net::local::BUS_ID,
        move |subscribe: net::local::Subscribe| {
            let topic = subscribe.topic().to_owned();
            let bcast = BCAST.clone();

            async move {
                log::debug!("NET: Subscribe to broadcast topic {}", topic);
                let (is_new, id) = bcast.add(subscribe).await;
                if is_new {
                    log::debug!("NET: Created new topic: {}", topic);
                }
                Ok(id)
            }
        },
    );

    let bcast_service_id = <SendBroadcastMessage<()> as RpcMessage>::ID;
    let _ = local_bus::subscribe(
        &format!("{}/{}", net::local::BUS_ID, bcast_service_id),
        move |caller: &str, addr: &str, msg: &[u8]| {
            broadcast_handler(client.clone(), caller, addr, msg, broadcast_size)
        },
        (),
    );
}

/// Handle incoming forward messages
fn forward_handler(
    client: Client,
    receiver: ForwardReceiver,
    state: State,
) -> impl Future<Output = ()> + Unpin + 'static {
    // Takes stream of generic packets, reads sender NodeId and translates
    // into stream designated to handle this specific Node.
    UnboundedReceiverStream::new(receiver)
        .for_each(move |fwd| {
            let state = state.clone();
            let client = client.clone();
            async move {
                let key = (fwd.node_id, fwd.transport);
                let route = match {
                    let inner = state.inner.borrow();
                    inner.routes.get(&key).cloned()
                } {
                    Some(route) => {
                        route.update_authentication(fwd.authenticated_identities.clone());
                        route
                    }
                    None => {
                        let state = state.clone();
                        let (tx, rx) = forward_channel(fwd.transport);
                        let route = NetRoute::new(tx, fwd.authenticated_identities.clone());
                        {
                            let mut inner = state.inner.borrow_mut();
                            inner.routes.insert(key, route.clone());
                        }
                        tokio::task::spawn_local(inbound_handler(
                            client.clone(),
                            rx,
                            fwd.node_id,
                            fwd.transport,
                            state,
                            route.authenticated_identities.clone(),
                        ));
                        route
                    }
                };
                let mut tx = route.sender;

                log::trace!(
                    "Net: received forward ({}) packet ({} B) from [{}]",
                    fwd.transport,
                    fwd.payload.len(),
                    fwd.node_id
                );

                tokio::task::spawn_local(async move {
                    if tx.send(fwd.payload).await.is_err() {
                        log::debug!("Net routing error: channel closed for [{}]", fwd.node_id);
                        state.remove_sink(&key);
                    }
                });
            }
        })
        .boxed_local()
}

fn forward_channel<'a>(
    transport: TransportType,
) -> (mpsc::Sender<Payload>, LocalBoxStream<'a, Payload>) {
    let (tx, rx) = mpsc::channel(8);
    let rx = if transport == TransportType::Reliable || transport == TransportType::Transfer {
        PrefixedStream::new(rx)
            .inspect_err(|e| log::debug!("Prefixed stream error: {e}"))
            .filter_map(|r| async move { r.ok().map(Payload::from) })
            .boxed_local()
    } else {
        rx.boxed_local()
    };
    (tx, rx)
}

/// Forward node GSB messages from the network to the local bus
fn inbound_handler(
    client: Client,
    rx: impl Stream<Item = Payload> + 'static,
    remote_id: NodeId,
    transport: TransportType,
    state: State,
    authenticated_identities: Rc<RefCell<Option<AuthenticatedIdentities>>>,
) -> impl Future<Output = ()> + Unpin + 'static {
    StreamExt::for_each(rx, move |payload| {
        let state = state.clone();
        let authenticated_identities = authenticated_identities.borrow().clone();
        log::trace!(
            "local bus handler -> inbound message ({} B) from [{remote_id}]",
            payload.len()
        );

        let client = client.clone();
        async move {
            match codec::decode_message(payload.as_ref()) {
                Ok(Some(GsbMessage::CallRequest(request @ ya_sb_proto::CallRequest { .. }))) => {
                    if request.no_reply {
                        handle_push(client, request, remote_id, state, authenticated_identities)
                            .await
                    } else {
                        handle_request(
                            client,
                            request,
                            remote_id,
                            state,
                            transport,
                            authenticated_identities,
                        )
                        .await
                    }
                }
                Ok(Some(GsbMessage::CallReply(reply @ ya_sb_proto::CallReply { .. }))) => {
                    handle_reply(client, reply, remote_id, state, authenticated_identities).await
                }
                Ok(Some(GsbMessage::BroadcastRequest(
                    request @ ya_sb_proto::BroadcastRequest { .. },
                ))) => {
                    handle_broadcast(client, request, remote_id, state, authenticated_identities)
                        .await
                }
                Ok(None) => {
                    log::trace!("Received a partial message from {remote_id}");
                    Ok(())
                }
                Err(err) => anyhow::bail!("Received message error: {}", err),
                _ => anyhow::bail!("unexpected message type"),
            }
        }
        .then(|result| async move {
            if let Err(e) = result {
                log::debug!("ingress message error: {}", e)
            }
        })
    })
    .boxed_local()
}

/// Forward messages from the network to the local bus
async fn handle_push(
    client: Client,
    request: ya_sb_proto::CallRequest,
    remote_id: NodeId,
    state: State,
    authenticated_identities: Option<AuthenticatedIdentities>,
) -> anyhow::Result<()> {
    let caller_id = parse_caller_id(&request.caller)?;
    authorize_published_identity(
        &client,
        &state,
        caller_id,
        remote_id,
        "caller",
        authenticated_identities.as_deref(),
    )
    .await?;

    let address = request.address;
    let request_id = request.request_id;

    log::debug!("Handle push request {request_id} to {address} from {remote_id}");

    let fut = match state.get_public_service(address.as_str()) {
        Some(address) => {
            log::trace!("Handle push request: calling: {address}");
            local_bus::push(&address, &request.caller, &request.data)
        }
        None => {
            log::trace!("Handle push request failed: unknown address: {address}");
            let err = Error::GsbBadRequest(format!("Unknown address: {address}"));
            return Err(err.into());
        }
    };

    tokio::task::spawn_local(async move {
        let _ = fut.await;
        log::debug!("Handled push request: {request_id} from: {caller_id}");
    });

    Ok(())
}

/// Forward messages from the network to the local bus
async fn handle_request(
    client: Client,
    request: ya_sb_proto::CallRequest,
    remote_id: NodeId,
    state: State,
    transport: TransportType,
    authenticated_identities: Option<AuthenticatedIdentities>,
) -> anyhow::Result<()> {
    let caller_id = parse_caller_id(&request.caller)?;
    authorize_published_identity(
        &client,
        &state,
        caller_id,
        remote_id,
        "caller",
        authenticated_identities.as_deref(),
    )
    .await?;

    let address = request.address;
    let request_id = request.request_id;
    let request_id_chain = request_id.clone();
    let request_id_filter = request_id.clone();
    let request_id_sent = request_id.clone();

    log::debug!("Handle request {request_id} to {address} from {remote_id}");

    let eos = Rc::new(AtomicBool::new(false));
    let eos_map = eos.clone();

    let stream = match state.get_public_service(address.as_str()) {
        Some(address) => {
            log::trace!("Handle request: calling: {address}");
            local_bus::call_stream(&address, &request.caller, &request.data).left_stream()
        }
        None => {
            log::trace!("Handle request failed: unknown address: {address}");
            let err = Error::GsbBadRequest(format!("Unknown address: {address}"));
            futures::stream::once(futures::future::err(err)).right_stream()
        }
    }
    .map(move |result| match result {
        Ok(chunk) => match chunk {
            ResponseChunk::Full(v) => {
                eos_map.store(true, Relaxed);
                match codec::decode_reply(v) {
                    Ok(v) => codec::reply_ok(request_id.clone(), ResponseChunk::Full(v)),
                    Err(err) => codec::reply_err(request_id.clone(), err),
                }
            }
            chunk => codec::reply_ok(request_id.clone(), chunk),
        },
        Err(err) => {
            eos_map.store(true, Relaxed);
            codec::reply_err(request_id.clone(), err)
        }
    })
    .chain(futures::stream::poll_fn(move |_| {
        if eos.load(Relaxed) {
            Poll::Ready(None)
        } else {
            eos.store(true, Relaxed);
            Poll::Ready(Some(codec::reply_eos(request_id_chain.clone())))
        }
    }))
    .filter_map(move |reply| {
        let filtered = match codec::encode_message(reply) {
            Ok(vec) => {
                log::debug!(
                    "Handle request {request_id_filter}: reply chunk ({} B)",
                    vec.len()
                );
                Some(Ok::<Vec<u8>, mpsc::SendError>(vec))
            }
            Err(e) => {
                log::debug!("Handle request: encode reply error: {e}");
                None
            }
        };
        async move { filtered }
    });

    tokio::task::spawn_local(
        async move {
            let mut sink = state.forward_sink(client, caller_id, transport).await?;
            let mut stream = Box::pin(stream);

            //stream.forward(sink).await?;
            while let Some(item) = stream.next().await {
                sink.send(item?.into()).await.ok();
                log::debug!("Handled request: {request_id_sent} from: {caller_id}");
            }

            Ok::<_, anyhow::Error>(())
        }
        .then(move |result| async move {
            if let Err(e) = result {
                log::debug!("Replying to [{caller_id}] - forward error: {e}");
            }
        }),
    );

    Ok(())
}

/// Forward replies from the network to the local bus
async fn handle_reply(
    client: Client,
    reply: ya_sb_proto::CallReply,
    remote_id: NodeId,
    state: State,
    authenticated_identities: Option<AuthenticatedIdentities>,
) -> anyhow::Result<()> {
    let full = reply.reply_type == ya_sb_proto::CallReplyType::Full as i32;

    log::debug!(
        "Handle reply from node {remote_id} (full: {full}, code: {}, id: {}) {} B",
        reply.code,
        reply.request_id,
        reply.data.len(),
    );

    let mut request = match {
        let inner = state.inner.borrow();
        inner.requests.get(&reply.request_id).cloned()
    } {
        Some(request) => {
            authorize_published_identity(
                &client,
                &state,
                request.remote_id,
                remote_id,
                "reply sender",
                authenticated_identities.as_deref(),
            )
            .await?;

            if full {
                let mut inner = state.inner.borrow_mut();
                inner.requests.remove(&reply.request_id);
            }
            request
        }
        None => anyhow::bail!("invalid reply request id: {}", reply.request_id),
    };

    let request_id = reply.request_id.clone();
    let data = if reply.code == CallReplyCode::CallReplyOk as i32 {
        reply.data
    } else {
        codec::encode_reply(reply).context("Unable to encode reply {request_id}")?
    };

    tokio::task::spawn_local(async move {
        let chunk = if full {
            ResponseChunk::Full(data)
        } else {
            ResponseChunk::Part(data)
        };
        if request.tx.send(chunk).await.is_err() {
            log::debug!("Failed to forward reply {request_id}: channel closed");
        }
    });

    Ok(())
}

/// Forward broadcasts from the network to the local bus
async fn handle_broadcast(
    client: Client,
    request: ya_sb_proto::BroadcastRequest,
    remote_id: NodeId,
    state: State,
    authenticated_identities: Option<AuthenticatedIdentities>,
) -> anyhow::Result<()> {
    let caller_id = parse_caller_id(&request.caller)?;
    authorize_published_identity(
        &client,
        &state,
        caller_id,
        remote_id,
        "broadcast caller",
        authenticated_identities.as_deref(),
    )
    .await?;

    log::trace!(
        "Received broadcast to topic {} from [{}].",
        &request.topic,
        &request.caller
    );

    let caller = caller_id.to_string();

    tokio::task::spawn_local(async move {
        let data = request.data;
        let topic = request.topic;

        for endpoint in BCAST
            .resolve(&topic)
            .await
            .into_iter()
            .map(|endpoint| endpoint.as_ref().to_string())
        {
            let bcast_service_id = <SendBroadcastMessage<()> as RpcMessage>::ID;
            let addr = format!("{}/{}", endpoint, bcast_service_id);

            log::trace!(
                "Forwarding broadcast from [{caller}] (topic: {topic}) to endpoint: {addr})"
            );
            if let Err(e) = local_bus::send(&addr, &caller, &data).await {
                log::debug!("Forwarding broadcast from [{caller}] to local endpoint error: {e}");
            }
        }
    });

    Ok(())
}

fn parse_caller_id(caller: &str) -> anyhow::Result<NodeId> {
    NodeId::from_str(caller).map_err(|e| anyhow!("Invalid caller id: {caller}: {e}"))
}

async fn authorize_published_identity(
    client: &Client,
    state: &State,
    identity_id: NodeId,
    remote_id: NodeId,
    field: &str,
    authenticated_identities: Option<&[NodeId]>,
) -> anyhow::Result<()> {
    authorize_published_identity_with(
        state,
        identity_id,
        remote_id,
        field,
        authenticated_identities,
        |remote_id| async move {
            let node = client.find_node(remote_id).await.with_context(|| {
                format!("Unable to resolve identities published by {remote_id}")
            })?;
            parse_published_identities(remote_id, node)
        },
    )
    .await
}

async fn authorize_published_identity_with<F, Fut>(
    state: &State,
    identity_id: NodeId,
    remote_id: NodeId,
    field: &str,
    authenticated_identities: Option<&[NodeId]>,
    resolve: F,
) -> anyhow::Result<()>
where
    F: FnOnce(NodeId) -> Fut,
    Fut: Future<Output = anyhow::Result<Vec<NodeId>>>,
{
    if let Some(identities) = authenticated_identities {
        let default_id = identities
            .first()
            .copied()
            .ok_or_else(|| anyhow!("Authenticated session for {remote_id} has no identities"))?;
        if default_id != remote_id {
            anyhow::bail!(
                "Authenticated session default identity {default_id} does not match remote {remote_id}"
            );
        }
        if !identities.contains(&identity_id) {
            if cfg!(feature = "encryption-strict") {
                anyhow::bail!(
                    "Invalid {field}: {identity_id} is not authenticated for remote {remote_id}"
                );
            }
        } else {
            return Ok(());
        }
    }

    if cfg!(feature = "encryption-strict") {
        anyhow::bail!("Missing authenticated identities for remote {remote_id}");
    }

    if identity_id == remote_id {
        return Ok(());
    }

    if let Some(authorized) = state.is_published_identity(remote_id, identity_id) {
        if authorized {
            return Ok(());
        }
        anyhow::bail!("Invalid {field}: {identity_id} is not published for remote {remote_id}");
    }

    let identities = resolve(remote_id).await?;
    let authorized = identities.contains(&identity_id);
    state.cache_published_identities(remote_id, identities);

    if !authorized {
        anyhow::bail!("Invalid {field}: {identity_id} is not published for remote {remote_id}");
    }

    Ok(())
}

fn parse_published_identities(
    remote_id: NodeId,
    node: ya_relay_client::model::Node,
) -> anyhow::Result<Vec<NodeId>> {
    let identities = node
        .identities
        .into_iter()
        .map(|identity| {
            let public_key = PublicKey::from_slice(&identity.public_key)
                .map_err(|_| anyhow!("Invalid public key published by remote {remote_id}"))?;
            let node_id = NodeId::from(public_key.address().as_ref());
            let published_node_id = NodeId::try_from(&identity.node_id)
                .map_err(|_| anyhow!("Invalid NodeId published by remote {remote_id}"))?;

            if node_id != published_node_id {
                anyhow::bail!("Mismatched NodeId published by remote {remote_id}");
            }

            Ok(node_id)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let default_id = identities
        .first()
        .copied()
        .ok_or_else(|| anyhow!("Remote {remote_id} published no identities"))?;
    if default_id != remote_id {
        anyhow::bail!("Remote {remote_id} published a different default identity {default_id}");
    }

    Ok(identities)
}

const PUBLISHED_IDENTITIES_TTL: Duration = Duration::from_secs(5 * 60);

struct PublishedIdentities {
    identities: Vec<NodeId>,
    valid_until: Instant,
}

#[derive(Clone)]
struct State {
    inner: Rc<RefCell<StateInner>>,
}

#[derive(Default)]
struct StateInner {
    requests: HashMap<String, Request<BusSender>>,
    routes: HashMap<NetSinkKey, NetRoute>,
    ids: HashSet<NodeId>,
    services: HashSet<String>,
    published_identities: HashMap<NodeId, PublishedIdentities>,
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

    async fn forward_sink(
        &self,
        client: Client,
        remote_id: NodeId,
        transport: TransportType,
    ) -> anyhow::Result<NetSinkKind> {
        let forward: NetSinkKind = match transport {
            TransportType::Unreliable => client.forward_unreliable(remote_id).await?,
            TransportType::Reliable => client.forward_reliable(remote_id).await?.framed(),
            TransportType::Transfer => client.forward_transfer(remote_id).await?.framed(),
        };

        // FIXME: yagna daemon doesn't handle connections; ya-relay-client does
        // if client.sessions.has_p2p_connection(remote_id).await {
        //     counter!("net.connections.p2p", 1)
        // } else {
        //     counter!("net.connections.relay", 1)
        // };

        Ok(forward)
    }

    fn get_public_service(&self, addr: &str) -> Option<String> {
        let inner = self.inner.borrow();
        RevPrefixes(addr)
            .find_map(|s| inner.services.get(s))
            .map(|s| addr.replacen(s, net::PUBLIC_PREFIX, 1))
    }

    fn remove_sink(&self, key: &NetSinkKey) {
        let mut inner = self.inner.borrow_mut();
        inner.routes.remove(key);
    }

    fn is_published_identity(&self, remote_id: NodeId, identity_id: NodeId) -> Option<bool> {
        let mut inner = self.inner.borrow_mut();
        let expired = inner
            .published_identities
            .get(&remote_id)
            .map(|entry| entry.valid_until <= Instant::now())
            .unwrap_or(false);

        if expired {
            inner.published_identities.remove(&remote_id);
            return None;
        }

        inner
            .published_identities
            .get(&remote_id)
            .map(|entry| entry.identities.contains(&identity_id))
    }

    fn cache_published_identities(&self, remote_id: NodeId, identities: Vec<NodeId>) {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();
        inner
            .published_identities
            .retain(|_, entry| entry.valid_until > now);
        inner.published_identities.insert(
            remote_id,
            PublishedIdentities {
                identities,
                valid_until: now + PUBLISHED_IDENTITIES_TTL,
            },
        );
    }
}

#[derive(Clone)]
struct Request<S: Clone> {
    #[allow(unused)]
    caller_id: NodeId,
    #[allow(unused)]
    remote_id: NodeId,
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

pub fn parse_net_to_addr(addr: &str) -> anyhow::Result<(NodeId, String)> {
    const ADDR_CONST: usize = 6;

    let mut it = addr.split('/').fuse().skip(1).peekable();
    let (prefix, to) = match (it.next(), it.next(), it.next()) {
        (Some("udp"), Some("net"), Some(to)) if it.peek().is_some() => ("/udp", to),
        (Some("net"), Some(to), Some(_)) => ("", to),
        (Some("transfer"), Some("net"), Some(to)) if it.peek().is_some() => ("/transfer", to),
        _ => anyhow::bail!("invalid net-to destination: {}", addr),
    };

    let to_id = to.parse::<NodeId>()?;
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

async fn bind_neighbourhood_bcast(client: Client) -> anyhow::Result<(), BindBroadcastError> {
    let bcast_address = format!("{}/{}", net::local::BUS_ID, NewNeighbour::TOPIC);
    crate::hybrid::bind_broadcast_with_caller(
        &bcast_address,
        move |caller, _msg: SendBroadcastMessage<NewNeighbour>| {
            let client = client.clone();
            async move {
                log::debug!(
                    "NewNeighbour notification fron [{caller}] - invalidating neighborhood cache."
                );
                client.invalidate_neighbourhood_cache().await;
                Ok(())
            }
        },
    )
    .await
}

pub async fn send_bcast_new_neighbour() {
    let node_id = crate::service::identities().await.unwrap().0;

    let net_type = { *NET_TYPE.read().unwrap() };
    if net_type == NetType::Hybrid {
        log::debug!("Broadcasting new neighbour notification.");
        if let Err(e) = broadcast(node_id, NewNeighbour {}).await {
            log::error!("Error broadcasting new neighbour: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case(
        "/net/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a/vpn/VpnControl",
        NodeId::from_str("0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a").unwrap();
        "net-to destination address")
    ]
    #[test_case(
        "/transfer/net/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a/vpn/VpnControl",
        NodeId::from_str("0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a").unwrap();
        "Address using heavy transfer channel")
    ]
    #[test_case(
        "/udp/net/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a/vpn/VpnControl",
        NodeId::from_str("0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a").unwrap();
        "Use unreliable transport protocol")
    ]
    fn test_parse_net_to_addr_positive(addr: &str, id: NodeId) {
        let (parsed_id, parsed_gsb) = parse_net_to_addr(addr).unwrap();
        assert_eq!(id, parsed_id);
        assert_eq!(addr, parsed_gsb);
    }

    #[test_case(
        "/net/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a";
        "net-to destination address - empty path - no trailing slash")
    ]
    #[test_case(
        "/transfer/net/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a";
        "Address using heavy transfer channel - empty path - no trailing slash")
    ]
    #[test_case(
        "/udp/net/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a";
        "Use unreliable transport protocol - empty path - no trailing slash")
    ]
    #[test_case(
        "/net/0x9xxxxxc6fd02afeca110b9c32a21fb8ad899ee0a/vpn/VpnControl";
        "net-to destination address - invalid NodeId")
    ]
    #[test_case(
        "/transfer/net/0x95369fc6fdca110b9c32a21fb8ad899ee0a/vpn/VpnControl";
        "Address using heavy transfer channel - invalid NodeId")
    ]
    #[test_case(
        "/udp/net/0x95369fc6fd02afec32a21fb8ad899ee0a/vpn/VpnControl";
        "Use unreliable transport protocol - invalid NodeId")
    ]
    fn test_parse_net_to_addr_negative_cases(addr: &str) {
        assert!(parse_net_to_addr(addr).is_err())
    }

    #[test_case(
        "/from/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a/to/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7/vpn/VpnControl",
        NodeId::from_str("0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a").unwrap(),
        NodeId::from_str("0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7").unwrap(),
        "/net/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7/vpn/VpnControl";
        "from-to destination address")
    ]
    #[test_case(
        "/transfer/from/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a/to/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7/vpn/VpnControl",
        NodeId::from_str("0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a").unwrap(),
        NodeId::from_str("0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7").unwrap(),
        "/transfer/net/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7/vpn/VpnControl";
        "from-to heavy transfer channel")
    ]
    #[test_case(
        "/udp/from/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a/to/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7/vpn/VpnControl",
        NodeId::from_str("0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a").unwrap(),
        NodeId::from_str("0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7").unwrap(),
        "/udp/net/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7/vpn/VpnControl";
        "from-to unreliable transport protocol")
    ]
    fn test_parse_from_to_addr_positive(addr: &str, from: NodeId, to: NodeId, remote_addr: &str) {
        let (parsed_from, parsed_to, parsed_gsb) = parse_from_to_addr(addr).unwrap();
        assert_eq!(parsed_from, from);
        assert_eq!(parsed_to, to);
        assert_eq!(remote_addr, parsed_gsb);
    }

    #[test_case(
        "/from/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a/to/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7";
        "from-to destination address - empty path - no trailing slash")
    ]
    #[test_case(
        "/transfer/from/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a/to/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7";
        "from-to heavy transfer channel - empty path - no trailing slash")
    ]
    #[test_case(
        "/udp/from/0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a/to/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7";
        "from-to unreliable transport protocol - empty path - no trailing slash")
    ]
    #[test_case(
        "/from/0x9xxxxxc6fd02afeca110b9c32a21fb8ad899ee0a/to/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7/vpn/VpnControl";
        "from-to destination address - invalid NodeId")
    ]
    #[test_case(
        "/transfer/from/0x95369fc6fdca110b9c32a21fb8ad899ee0a/to/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7/vpn/VpnControl";
        "from-to heavy transfer channel - invalid NodeId")
    ]
    #[test_case(
        "/udp/from/0x95369fc6fd02afec32a21fb8ad899ee0a/to/0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7/vpn/VpnControl";
        "from-to unreliable transport protocol - invalid NodeId")
    ]
    fn test_parse_from_to_addr_negative_cases(addr: &str) {
        assert!(parse_from_to_addr(addr).is_err())
    }

    fn published_node(seeds: &[u8]) -> (ya_relay_client::model::Node, Vec<NodeId>) {
        use ethsign::SecretKey;

        let mut node = ya_relay_client::model::Node::default();
        let mut node_ids = Vec::new();

        for seed in seeds {
            let public_key = SecretKey::from_raw(&[*seed; 32]).unwrap().public();
            let node_id = NodeId::from(public_key.address().as_ref());

            node.identities.push(Default::default());
            let identity = node.identities.last_mut().unwrap();
            identity.node_id = node_id.into_array().to_vec();
            identity.public_key = public_key.bytes().to_vec();
            node_ids.push(node_id);
        }

        (node, node_ids)
    }

    fn empty_state() -> State {
        State::new(Vec::<NodeId>::new(), HashSet::new())
    }

    #[test]
    fn test_net_route_authentication_can_only_be_downgraded() {
        let (sender, _receiver) = mpsc::channel(1);
        let (_node, identities) = published_node(&[1, 2]);
        let identities: AuthenticatedIdentities = identities.into();

        let authenticated = NetRoute::new(sender.clone(), Some(identities.clone()));
        authenticated.update_authentication(None);
        authenticated.update_authentication(Some(identities.clone()));
        assert!(authenticated.authenticated_identities.borrow().is_none());

        let legacy = NetRoute::new(sender, None);
        legacy.update_authentication(Some(identities));
        assert!(legacy.authenticated_identities.borrow().is_none());
    }

    #[cfg(not(feature = "encryption-strict"))]
    #[test]
    fn test_published_identity_allows_exact_remote_without_discovery() {
        let remote = NodeId::from_str("0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a").unwrap();
        let state = empty_state();

        let result = futures::executor::block_on(authorize_published_identity_with(
            &state,
            remote,
            remote,
            "caller",
            None,
            |_| async { unreachable!("exact identity must not trigger discovery") },
        ));

        assert!(result.is_ok());
    }

    #[test]
    fn test_reply_sender_allows_default_identity_for_alias_target() {
        let state = empty_state();
        let (_node, identities) = published_node(&[1, 2]);
        let default_id = identities[0];
        let alias = identities[1];

        let result = futures::executor::block_on(authorize_published_identity_with(
            &state,
            alias,
            default_id,
            "reply sender",
            Some(&identities),
            |_| async { unreachable!("authenticated identity must not trigger discovery") },
        ));

        assert!(result.is_ok());
    }

    #[cfg(not(feature = "encryption-strict"))]
    #[test]
    fn test_published_identity_rejects_foreign_alias_and_caches_result() {
        let state = empty_state();
        let (_node, identities) = published_node(&[1, 2, 3]);
        let default_id = identities[0];
        let own_alias = identities[1];
        let foreign_alias = identities[2];
        let published = vec![default_id, own_alias];

        let first = futures::executor::block_on(authorize_published_identity_with(
            &state,
            foreign_alias,
            default_id,
            "caller",
            None,
            move |_| async move { Ok(published) },
        ));
        let second = futures::executor::block_on(authorize_published_identity_with(
            &state,
            foreign_alias,
            default_id,
            "caller",
            None,
            |_| async { unreachable!("cached rejection must not trigger discovery") },
        ));

        assert!(first.is_err());
        assert!(second.is_err());
    }

    #[cfg(not(feature = "encryption-strict"))]
    #[test]
    fn test_expired_published_identity_cache_is_refreshed() {
        let state = empty_state();
        let (_node, identities) = published_node(&[1, 2]);
        let default_id = identities[0];
        let alias = identities[1];

        state.inner.borrow_mut().published_identities.insert(
            default_id,
            PublishedIdentities {
                identities: vec![default_id, alias],
                valid_until: Instant::now() - Duration::from_secs(1),
            },
        );

        let result = futures::executor::block_on(authorize_published_identity_with(
            &state,
            alias,
            default_id,
            "caller",
            None,
            move |_| async move { Ok(vec![default_id]) },
        ));

        assert!(result.is_err());
    }

    #[cfg(not(feature = "encryption-strict"))]
    #[test]
    fn test_compatibility_mode_resolves_unproved_alias() {
        let state = empty_state();
        let (_node, identities) = published_node(&[1, 2, 3]);
        let default_id = identities[0];
        let legacy_alias = identities[2];
        let authenticated = &identities[..2];
        let published = identities.clone();

        let result = futures::executor::block_on(authorize_published_identity_with(
            &state,
            legacy_alias,
            default_id,
            "caller",
            Some(authenticated),
            move |_| async move { Ok(published) },
        ));

        assert!(result.is_ok());
    }

    #[cfg(feature = "encryption-strict")]
    #[test]
    fn test_strict_mode_rejects_unproved_alias_without_discovery() {
        let state = empty_state();
        let (_node, identities) = published_node(&[1, 2, 3]);
        let default_id = identities[0];
        let unproved_alias = identities[2];
        let authenticated = &identities[..2];

        let result = futures::executor::block_on(authorize_published_identity_with(
            &state,
            unproved_alias,
            default_id,
            "caller",
            Some(authenticated),
            |_| async { unreachable!("strict rejection must not trigger discovery") },
        ));

        assert!(result.is_err());
    }

    #[test]
    fn test_authenticated_identities_require_matching_default() {
        let state = empty_state();
        let (_node, identities) = published_node(&[1, 2]);

        let result = futures::executor::block_on(authorize_published_identity_with(
            &state,
            identities[0],
            identities[1],
            "caller",
            Some(&identities),
            |_| async { unreachable!("invalid authentication must not trigger discovery") },
        ));

        assert!(result.is_err());
    }

    #[cfg(feature = "encryption-strict")]
    #[test]
    fn test_strict_mode_rejects_missing_authenticated_identities() {
        let state = empty_state();
        let remote = NodeId::from_str("0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a").unwrap();

        let result = futures::executor::block_on(authorize_published_identity_with(
            &state,
            remote,
            remote,
            "caller",
            None,
            |_| async { unreachable!("strict rejection must not trigger discovery") },
        ));

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_published_identities_validates_default_and_aliases() {
        let (node, identities) = published_node(&[1, 2]);

        let published = parse_published_identities(identities[0], node).unwrap();

        assert_eq!(published, identities);
    }

    #[test]
    fn test_parse_published_identities_rejects_wrong_default() {
        let (node, identities) = published_node(&[1, 2]);

        assert!(parse_published_identities(identities[1], node).is_err());
    }

    #[test]
    fn test_parse_published_identities_rejects_mismatched_node_id() {
        let (mut node, identities) = published_node(&[1, 2]);
        node.identities[1].node_id = identities[0].into_array().to_vec();

        assert!(parse_published_identities(identities[0], node).is_err());
    }

    #[test]
    fn test_parse_published_identities_rejects_empty_response() {
        let remote = NodeId::from_str("0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a").unwrap();

        assert!(parse_published_identities(remote, Default::default()).is_err());
    }
}
