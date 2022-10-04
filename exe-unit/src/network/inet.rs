use std::collections::HashMap;
use std::convert::TryFrom;
use std::future::Future;
use std::iter::FromIterator;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use actix::prelude::*;
use anyhow::anyhow;
use bytes::{Bytes, BytesMut};
use futures::prelude::stream::{SplitSink, SplitStream};
use futures::{FutureExt, Sink, SinkExt, StreamExt};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::RwLock;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_util::codec::{BytesCodec, Framed};
use tokio_util::udp::UdpFramed;

use net::connection::{Connection, ConnectionMeta};
use net::interface::tap_iface;
use net::smoltcp::wire::{IpAddress, IpCidr, IpEndpoint};
use net::socket::SocketDesc;
use net::{EgressReceiver, IngressEvent, IngressReceiver};
use net::{Error as NetError, Protocol};

use ya_runtime_api::server::{CreateNetwork, NetworkInterface, RuntimeService};
use ya_utils_networking::vpn::common::{ntoh, DEFAULT_MAX_FRAME_SIZE};
use ya_utils_networking::vpn::stack as net;
use ya_utils_networking::vpn::stack::smoltcp::wire::{EthernetAddress, HardwareAddress};
use ya_utils_networking::vpn::stack::NetworkConfig;
use ya_utils_networking::vpn::{
    EtherFrame, EtherType, IpPacket, PeekPacket, SocketEndpoint, TcpPacket, UdpPacket,
};

use crate::manifest::UrlValidator;
use crate::message::Shutdown;
use crate::network;
use crate::network::{Endpoint, RxBuffer};
use crate::{Error, Result};

const IP4_ADDRESS: std::net::Ipv4Addr = std::net::Ipv4Addr::new(9, 0, 0x0d, 0x01);
const IP6_ADDRESS: std::net::Ipv6Addr = IP4_ADDRESS.to_ipv6_mapped();
const TCP_KEEP_ALIVE: Duration = Duration::from_secs(30);
const DEFAULT_PREFIX_LEN: u8 = 24;

type TcpSender = Arc<Mutex<SplitSink<Framed<TcpStream, BytesCodec>, Bytes>>>;
type UdpSender = Arc<Mutex<SplitSink<UdpFramed<BytesCodec>, (Bytes, SocketAddr)>>>;
type TcpReceiver = SplitStream<Framed<TcpStream, BytesCodec>>;
type UdpReceiver = SplitStream<UdpFramed<BytesCodec>>;
type TransportKey = (
    Option<Protocol>,
    Box<[u8]>, // local address bytes
    Option<u16>,
    Box<[u8]>, // remote address bytes
    Option<u16>,
);

pub(crate) async fn start_inet<R: RuntimeService>(
    service: &R,
    filter: Option<UrlValidator>,
) -> Result<Addr<Inet>> {
    use ya_runtime_api::server::Network;

    log::info!("Starting outbound network service...");

    let ip4_net = ipnet::Ipv4Net::new(IP4_ADDRESS, DEFAULT_PREFIX_LEN).unwrap();
    // let ip6_net = ipnet::Ipv6Net::new(IP6_ADDRESS, 128 - DEFAULT_PREFIX_LEN).unwrap();

    let ip4_addr = ip4_net.hosts().nth(1).unwrap();
    // let ip6_addr = ip6_net.hosts().skip(1).next().unwrap();

    let networks = [
        Network {
            addr: IP4_ADDRESS.to_string(),
            gateway: IP4_ADDRESS.to_string(),
            mask: ip4_net.netmask().to_string(),
            if_addr: ip4_addr.to_string(),
        },
        // Network {
        //     addr: ip6_repr(ip6_net.network()),
        //     gateway: ip6_repr(IP6_ADDRESS),
        //     mask: ip6_repr(ip6_net.netmask()),
        //     if_addr: ip6_repr(ip6_addr),
        // },
    ]
    .to_vec();

    let response = service
        .create_network(CreateNetwork {
            networks,
            hosts: Default::default(),
            interface: NetworkInterface::Inet as i32,
        })
        .await
        .map_err(|e| Error::Other(format!("initialization error: {:?}", e)))?;

    let endpoint = match response.endpoint {
        Some(endpoint) => Endpoint::connect(endpoint).await?,
        None => return Err(Error::Other("endpoint already connected".into())),
    };

    Ok(Inet::new(endpoint, filter).start())
}

pub(crate) struct Inet {
    network: net::Network,
    endpoint: Endpoint,
    proxy: Proxy,
}

impl Inet {
    pub fn new(endpoint: Endpoint, filter: Option<UrlValidator>) -> Self {
        let network = Self::create_network();
        let proxy = Proxy::new(network.clone(), filter);
        Self {
            network,
            endpoint,
            proxy,
        }
    }

    fn create_network() -> net::Network {
        let config = Rc::new(NetworkConfig {
            max_transmission_unit: DEFAULT_MAX_FRAME_SIZE,
            buffer_size_multiplier: 4,
        });

        let ethernet_addr = loop {
            let addr = EthernetAddress(rand::random());
            if addr.is_unicast() {
                break addr;
            }
        };

        let iface = tap_iface(
            HardwareAddress::Ethernet(ethernet_addr),
            config.max_transmission_unit,
        );
        let stack = net::Stack::new(iface, config.clone());

        stack.add_address(IpCidr::new(IP4_ADDRESS.into(), 16));
        stack.add_address(IpCidr::new(IP6_ADDRESS.into(), 0));

        net::Network::new("inet", config, stack)
    }
}

impl Actor for Inet {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.network.spawn_local();

        let router = Router::new(self.network.clone(), self.proxy.clone());

        let endpoint_rx = match self.endpoint.rx.take() {
            Some(rx) => rx,
            None => {
                log::error!("[inet] local endpoint missing");
                ctx.stop();
                return;
            }
        };

        let ingress_rx = self
            .network
            .ingress_receiver()
            .expect("Ingress receiver already taken");

        let egress_rx = self
            .network
            .egress_receiver()
            .expect("Egress receiver already taken");

        inet_endpoint_egress_handler(endpoint_rx, router)
            .into_actor(self)
            .spawn(ctx);

        inet_ingress_handler(ingress_rx, self.proxy.clone())
            .into_actor(self)
            .spawn(ctx);

        inet_egress_handler(egress_rx, self.endpoint.tx.clone())
            .into_actor(self)
            .spawn(ctx);
    }

    fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
        self.network = Self::create_network();
        self.proxy = Proxy::new(self.network.clone(), self.proxy.filter.clone());

        log::info!("[inet] stopping service");
        Running::Stop
    }
}

impl Handler<Shutdown> for Inet {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, msg: Shutdown, ctx: &mut Context<Self>) -> Self::Result {
        log::info!("[inet] shutting down: {:?}", msg.0);
        ctx.stop();
        Ok(())
    }
}

async fn inet_endpoint_egress_handler(
    mut rx: Box<dyn Stream<Item = Result<Vec<u8>>> + Unpin>,
    router: Router,
) {
    let mut rx_buf = RxBuffer::default();

    while let Some(result) = rx.next().await {
        let received = match result {
            Ok(vec) => vec,
            Err(err) => return log::debug!("[inet] runtime -> inet error: {}", err),
        };

        for packet in rx_buf.process(received) {
            router.handle(&packet).await;

            log::debug!("[inet] runtime -> inet packet {:?}", packet);

            router.network.receive(packet);
            router.network.poll();
        }
    }
}

async fn inet_ingress_handler(rx: IngressReceiver, proxy: Proxy) {
    let mut rx = UnboundedReceiverStream::new(rx);
    while let Some(event) = rx.next().await {
        match event {
            IngressEvent::InboundConnection { desc } => log::debug!(
                "[inet] ingress: connection to {:?} ({}) from {:?}",
                desc.local,
                desc.protocol,
                desc.remote
            ),
            IngressEvent::Disconnected { desc } => {
                log::debug!(
                    "[inet] ingress: disconnect {:?} ({}) by {:?}",
                    desc.local,
                    desc.protocol,
                    desc.remote,
                );
                let _ = proxy.unbind(desc).await;
            }
            IngressEvent::Packet { payload, desc, .. } => {
                let key = (&desc).proxy_key().unwrap();

                if let Some(mut sender) = proxy.get(&key).await {
                    log::debug!("[inet] ingress proxy: send to {:?}", desc.local);

                    if let Err(e) = sender.send(Bytes::from(payload)).await {
                        log::debug!("[inet] ingress proxy: send error: {}", e);
                    }
                } else {
                    log::debug!("[inet] ingress proxy: no connection to {:?}", desc);
                }
            }
        }
    }

    log::debug!("[inet] ingress handler stopped");
}

async fn inet_egress_handler<E: std::fmt::Display>(
    rx: EgressReceiver,
    mut fwd: impl Sink<Result<Vec<u8>>, Error = E> + Unpin + 'static,
) {
    let mut rx = UnboundedReceiverStream::new(rx);
    while let Some(event) = rx.next().await {
        let mut frame = event.payload.into_vec();
        log::debug!("[inet] egress -> runtime packet {} B", frame.len());

        network::write_prefix(&mut frame);
        if let Err(e) = fwd.send(Ok(frame)).await {
            log::debug!("[inet] egress -> runtime error: {}", e);
        }
    }

    log::debug!("[inet] egress -> runtime handler stopped");
}

struct Router {
    network: net::Network,
    proxy: Proxy,
}

impl Router {
    fn new(network: net::Network, proxy: Proxy) -> Self {
        Self { network, proxy }
    }

    async fn handle(&self, frame: &Vec<u8>) {
        match EtherFrame::peek_type(frame.as_slice()) {
            Err(_) | Ok(EtherType::Arp) => return,
            _ => {}
        }
        let eth_payload = match EtherFrame::peek_payload(frame.as_slice()) {
            Ok(payload) => payload,
            _ => return,
        };
        let ip_packet = match IpPacket::peek(eth_payload) {
            Ok(_) => IpPacket::packet(eth_payload),
            _ => return,
        };

        match ip_packet_to_socket_desc(&ip_packet) {
            Ok(desc) => match self.proxy.bind(desc).await {
                Ok(handler) => {
                    tokio::task::spawn_local(handler);
                }
                Err(err) => {
                    log::debug!("[inet] router: connection error: {}", err);
                }
            },
            Err(error) => match error {
                Error::Net(NetError::ProtocolNotSupported(_)) => {}
                error => log::debug!("[inet] router: {}", error),
            },
        }
    }
}

fn ip_packet_to_socket_desc(ip_packet: &IpPacket) -> Result<SocketDesc> {
    let protocol = match Protocol::try_from(ip_packet.protocol()) {
        Ok(protocol) => protocol,
        _ => return Err(NetError::ProtocolUnknown.into()),
    };

    let (sender_port, listen_port) = match protocol {
        Protocol::Tcp => {
            TcpPacket::peek(ip_packet.payload())?;
            let pkt = TcpPacket::packet(ip_packet.payload());
            (pkt.src_port(), pkt.dst_port())
        }
        Protocol::Udp => {
            UdpPacket::peek(ip_packet.payload())?;
            let pkt = UdpPacket::packet(ip_packet.payload());
            (pkt.src_port(), pkt.dst_port())
        }
        _ => return Err(NetError::ProtocolNotSupported(protocol.to_string()).into()),
    };

    let sender_ip = match ntoh(ip_packet.src_address()) {
        Some(ip) => IpAddress::from(ip),
        None => {
            return Err(NetError::IpAddrMalformed(format!(
                "invalid sender IP: {:?}",
                ip_packet.src_address()
            ))
            .into());
        }
    };

    let listen_ip = match ntoh(ip_packet.dst_address()) {
        Some(ip) => IpAddress::from(ip),
        None => {
            return Err(NetError::IpAddrMalformed(format!(
                "invalid remote IP: {:?}",
                ip_packet.dst_address()
            ))
            .into());
        }
    };

    Ok(SocketDesc {
        protocol,
        local: SocketEndpoint::Ip((listen_ip, listen_port).into()),
        remote: SocketEndpoint::Ip((sender_ip, sender_port).into()),
    })
}

#[derive(Clone)]
struct Proxy {
    state: Arc<RwLock<ProxyState>>,
    filter: Option<UrlValidator>,
}

struct ProxyState {
    network: net::Network,
    remotes: HashMap<TransportKey, TransportSender>,
}

impl Proxy {
    fn new(network: net::Network, filter: Option<UrlValidator>) -> Self {
        let state = ProxyState {
            network,
            remotes: Default::default(),
        };
        Self {
            state: Arc::new(RwLock::new(state)),
            filter,
        }
    }

    async fn exists(&self, key: &TransportKey) -> bool {
        let state = self.state.read().await;
        state.remotes.contains_key(key)
    }

    async fn get(&self, key: &TransportKey) -> Option<TransportSender> {
        let state = self.state.read().await;
        state.remotes.get(key).cloned()
    }

    async fn bind(&self, desc: SocketDesc) -> Result<impl Future<Output = ()> + 'static> {
        let meta = ConnectionMeta::try_from(desc)?;

        log::debug!(
            "[inet] proxy conn: bind {} ({}) and {}",
            meta.local,
            meta.protocol,
            meta.remote
        );

        let key = (&meta).proxy_key()?;

        let (network, handle) = {
            let state = self.state.write().await;
            match state.network.get_bound(desc.protocol, desc.local) {
                Some(handle) => (state.network.clone(), handle),
                None => {
                    log::debug!("[inet] bind to {:?}", desc);

                    let ip_cidr = IpCidr::new(meta.local.addr, 0);
                    state.network.stack.add_address(ip_cidr);
                    let handle = state.network.bind(meta.protocol, meta.local)?;
                    (state.network.clone(), handle)
                }
            }
        };

        if self.exists(&key).await {
            return Ok(async move {
                log::debug!(
                    "[inet] proxy conn: already connected to {} ({}) from {}",
                    meta.local,
                    meta.protocol,
                    meta.remote
                );
            }
            .left_future());
        }

        log::debug!("[inet] connect to {:?}", desc);

        let (ip, port) = (conv_ip_addr(meta.local.addr)?, meta.local.port);
        if let Some(ref filter) = self.filter {
            filter.validate(meta.protocol, ip, port)?;
        }

        let (tx, mut rx) = match meta.protocol {
            Protocol::Tcp => inet_tcp_proxy(ip, port).await?,
            Protocol::Udp => inet_udp_proxy(ip, port).await?,
            other => return Err(NetError::ProtocolNotSupported(other.to_string()).into()),
        };

        let conn = Connection { handle, meta };
        let proxy = self.clone();

        let mut state = self.state.write().await;
        state.remotes.insert(key, tx);

        Ok(async move {
            while let Some(bytes) = rx.next().await {
                let vec = match bytes {
                    Ok(bytes) => Vec::from_iter(bytes.into_iter()),
                    Err(err) => {
                        log::debug!("[inet] proxy conn: bytes error: {}", err);
                        continue;
                    }
                };

                log::debug!(
                    "[inet] proxy conn: forward received bytes ({} B) to {:?}",
                    vec.len(),
                    conn
                );

                match network.send(vec, conn) {
                    Ok(fut) => {
                        if let Err(e) = fut.await {
                            log::debug!("[inet] proxy conn: forward error: {}", e);
                        }
                    }
                    Err(e) => {
                        log::debug!("[inet] proxy conn: send error: {}", e);
                    }
                };
            }

            let _ = proxy.unbind(desc).await;
            log::debug!("[inet] proxy conn closed: {:?}", desc);
        }
        .right_future())
    }

    async fn unbind(&self, desc: SocketDesc) -> Result<()> {
        log::debug!("[inet] proxy unbind: {:?}", desc);

        let meta = ConnectionMeta::try_from(desc)?;
        let key = (&meta).proxy_key()?;
        let mut inner = self.state.write().await;

        if let Some(mut conn) = inner.remotes.remove(&key) {
            let _ = conn.close().await;
        }

        Ok(())
    }
}

async fn inet_tcp_proxy<'a>(ip: IpAddr, port: u16) -> Result<(TransportSender, TransportReceiver)> {
    log::debug!("[inet] connecting TCP to {}:{}", ip, port);

    let tcp_stream = tcp_connect(&SocketAddr::new(ip, port), None)
        .map_err(|e| Error::Other(e.to_string()))?
        .await
        .map_err(|e| Error::Other(e.to_string()))?;
    let _ = tcp_stream.set_nodelay(true);

    let stream = Framed::with_capacity(tcp_stream, BytesCodec::new(), DEFAULT_MAX_FRAME_SIZE);
    let (tx, rx) = stream.split();
    Ok((
        TransportSender::Tcp(Arc::new(Mutex::new(tx))),
        TransportReceiver::Tcp(rx),
    ))
}

// Copied from: https://github.com/hyperium/hyper/blob/055b4e7ea6bd22859c20d60776b0c8f20d27498e/src/client/connect/http.rs#L588-L673
fn tcp_connect(
    addr: &SocketAddr,
    connect_timeout: Option<Duration>,
) -> anyhow::Result<impl Future<Output = anyhow::Result<TcpStream>>> {
    // TODO(eliza): if Tokio's `TcpSocket` gains support for setting the
    // keepalive timeout, it would be nice to use that instead of socket2,
    // and avoid the unsafe `into_raw_fd`/`from_raw_fd` dance...
    use socket2::{Domain, Protocol, Socket, TcpKeepalive, Type};

    let domain = Domain::for_address(*addr);
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))
        .map_err(|e| anyhow!("tcp open error: {}", e))?;

    // When constructing a Tokio `TcpSocket` from a raw fd/socket, the user is
    // responsible for ensuring O_NONBLOCK is set.
    socket
        .set_nonblocking(true)
        .map_err(|e| anyhow!("tcp set_nonblocking error: {}", e))?;

    let conf = TcpKeepalive::new().with_time(TCP_KEEP_ALIVE);
    if let Err(e) = socket.set_tcp_keepalive(&conf) {
        log::warn!("tcp set_keepalive error: {}", e);
    }

    bind_local_address(&socket, addr, &None, &None)
        .map_err(|e| anyhow!("tcp bind local error: {}", e))?;

    #[cfg(unix)]
    let socket = unsafe {
        // Safety: `from_raw_fd` is only safe to call if ownership of the raw
        // file descriptor is transferred. Since we call `into_raw_fd` on the
        // socket2 socket, it gives up ownership of the fd and will not close
        // it, so this is safe.
        use std::os::unix::io::{FromRawFd, IntoRawFd};
        TcpSocket::from_raw_fd(socket.into_raw_fd())
    };
    #[cfg(windows)]
    let socket = unsafe {
        // Safety: `from_raw_socket` is only safe to call if ownership of the raw
        // Windows SOCKET is transferred. Since we call `into_raw_socket` on the
        // socket2 socket, it gives up ownership of the SOCKET and will not close
        // it, so this is safe.
        use std::os::windows::io::{FromRawSocket, IntoRawSocket};
        TcpSocket::from_raw_socket(socket.into_raw_socket())
    };

    let connect = socket.connect(*addr);
    Ok(async move {
        match connect_timeout {
            Some(dur) => match tokio::time::timeout(dur, connect).await {
                Ok(Ok(s)) => Ok(s),
                Ok(Err(e)) => Err(e.into()),
                Err(_elapsed) => Err(anyhow!("connection timeout")),
            },
            None => Ok(connect.await?),
        }
        .map_err(|e| anyhow!("tcp connect error: {}", e))
    })
}

fn bind_local_address(
    socket: &socket2::Socket,
    dst_addr: &SocketAddr,
    local_addr_ipv4: &Option<Ipv4Addr>,
    local_addr_ipv6: &Option<Ipv6Addr>,
) -> anyhow::Result<()> {
    match (*dst_addr, local_addr_ipv4, local_addr_ipv6) {
        (SocketAddr::V4(_), Some(addr), _) => {
            socket.bind(&SocketAddr::new((*addr).into(), 0).into())?;
        }
        (SocketAddr::V6(_), _, Some(addr)) => {
            socket.bind(&SocketAddr::new((*addr).into(), 0).into())?;
        }
        _ => {
            if cfg!(windows) {
                // Windows requires a socket be bound before calling connect
                let any: SocketAddr = match *dst_addr {
                    SocketAddr::V4(_) => ([0, 0, 0, 0], 0).into(),
                    SocketAddr::V6(_) => ([0, 0, 0, 0, 0, 0, 0, 0], 0).into(),
                };
                socket.bind(&any.into())?;
            }
        }
    }

    Ok(())
}

async fn inet_udp_proxy<'a>(ip: IpAddr, port: u16) -> Result<(TransportSender, TransportReceiver)> {
    log::debug!("[inet] opening UDP socket with {}:{}", ip, port);

    let socket_addr: SocketAddr = (ip, port).into();
    let udp_socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    udp_socket.connect(socket_addr).await?;

    let (tx, rx) = UdpFramed::new(udp_socket, BytesCodec::new()).split();
    Ok((
        TransportSender::Udp(Arc::new(Mutex::new(tx)), socket_addr),
        TransportReceiver::Udp(rx),
    ))
}

#[derive(Clone)]
enum TransportSender {
    Tcp(TcpSender),
    Udp(UdpSender, SocketAddr),
}

impl Sink<Bytes> for TransportSender {
    type Error = Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        match self.get_mut() {
            Self::Tcp(tcp) => {
                let mut guard = tcp.lock().unwrap();
                Pin::new(&mut (*guard)).poll_ready(cx).map_err(Error::from)
            }
            Self::Udp(udp, _) => {
                let mut guard = udp.lock().unwrap();
                Pin::new(&mut (*guard)).poll_ready(cx).map_err(Error::from)
            }
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Bytes) -> std::result::Result<(), Self::Error> {
        match self.get_mut() {
            Self::Tcp(tcp) => {
                let mut guard = tcp.lock().unwrap();
                Pin::new(&mut (*guard))
                    .start_send(item)
                    .map_err(Error::from)
            }
            Self::Udp(udp, addr) => {
                let mut guard = udp.lock().unwrap();
                Pin::new(&mut (*guard))
                    .start_send((item, *addr))
                    .map_err(Error::from)
            }
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        match self.get_mut() {
            Self::Tcp(tcp) => {
                let mut guard = tcp.lock().unwrap();
                Pin::new(&mut (*guard)).poll_flush(cx).map_err(Error::from)
            }
            Self::Udp(udp, _) => {
                let mut guard = udp.lock().unwrap();
                Pin::new(&mut (*guard)).poll_flush(cx).map_err(Error::from)
            }
        }
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        match self.get_mut() {
            Self::Tcp(tcp) => {
                let mut guard = tcp.lock().unwrap();
                Pin::new(&mut (*guard)).poll_close(cx).map_err(Error::from)
            }
            Self::Udp(udp, _) => {
                let mut guard = udp.lock().unwrap();
                Pin::new(&mut (*guard)).poll_close(cx).map_err(Error::from)
            }
        }
    }
}

impl Unpin for TransportSender {}

enum TransportReceiver {
    Tcp(TcpReceiver),
    Udp(UdpReceiver),
}

impl Stream for TransportReceiver {
    type Item = Result<BytesMut>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        match self.get_mut() {
            Self::Tcp(tcp) => Pin::new(tcp)
                .poll_next(cx)
                .map(|opt| opt.map(|res| res.map_err(Error::from))),
            Self::Udp(udp) => Pin::new(udp)
                .poll_next(cx)
                .map(|opt| opt.map(|res| res.map(|(b, _)| b).map_err(Error::from))),
        }
    }
}

impl Unpin for TransportReceiver {}

trait TransportKeyExt {
    fn proxy_key(self) -> Result<TransportKey>;

    fn proxy_key_mirror(self) -> Result<TransportKey>
    where
        Self: Sized,
    {
        let key = self.proxy_key()?;
        Ok((key.0, key.3, key.4, key.1, key.2))
    }
}

impl<'a> TransportKeyExt for &'a ConnectionMeta {
    fn proxy_key(self) -> Result<TransportKey> {
        Ok((
            Some(self.protocol),
            self.local.addr.as_bytes().into(),
            Some(self.local.port),
            self.remote.addr.as_bytes().into(),
            Some(self.remote.port),
        ))
    }
}

impl<'a> TransportKeyExt for &'a SocketDesc {
    fn proxy_key(self) -> Result<TransportKey> {
        let local = self.local.ip_endpoint()?;
        let remote = self.remote.ip_endpoint()?;

        Ok((
            Some(self.protocol),
            local.addr.as_bytes().into(),
            Some(local.port),
            remote.addr.as_bytes().into(),
            Some(remote.port),
        ))
    }
}

fn conv_ip_addr(addr: IpAddress) -> Result<IpAddr> {
    match addr {
        IpAddress::Ipv4(ipv4) => Ok(IpAddr::V4(ipv4.into())),
        IpAddress::Ipv6(ipv6) => Ok(IpAddr::V6(ipv6.into())),
        _ => Err(NetError::EndpointInvalid(IpEndpoint::from((addr, 0)).into()).into()),
    }
}

// fn ip6_repr(ip6: std::net::Ipv6Addr) -> String {
//     let mut result = String::with_capacity(8 * 4 + 7);
//     let octets = ip6.octets();
//
//     for (i, b) in octets.iter().enumerate() {
//         let sep = i % 2 == 1 && i != octets.len() - 1;
//         result = format!("{}{:02x?}{}", result, b, if sep { ":" } else { "" });
//     }
//     result
// }
