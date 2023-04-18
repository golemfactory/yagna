use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::{Buf, BytesMut};
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{BoxStream, Peekable};
use futures::{Stream, StreamExt};
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpSocket, UdpSocket};
use tokio::sync::mpsc;

use ya_runtime_api::deploy::ContainerEndpoint;
use ya_runtime_api::server::Network;
use ya_service_bus::{typed, typed::Endpoint as GsbEndpoint};
use ya_utils_networking::vpn::common::DEFAULT_MAX_FRAME_SIZE;
use ya_utils_networking::vpn::{network::DuoEndpoint};

use crate::error::Error;
use crate::state::DeploymentNetwork;
use crate::Result;

pub(crate) mod inet;
pub(crate) mod vpn;

type SocketChannel = (
    mpsc::UnboundedSender<Result<Vec<u8>>>,
    mpsc::UnboundedReceiver<Result<Vec<u8>>>,
);

const SOCKET_BUFFER_SIZE: usize = 2097152;
const BUFFER_SIZE: usize = DEFAULT_MAX_FRAME_SIZE * 4;

pub(crate) enum Endpoint {
    New {
        local: LocalEndpoint,
    },
    Connected {
        local: LocalEndpoint,
        remote: RemoteEndpoint,
    },
    Poisoned,
}

// FIXME: IPv6 support
impl Endpoint {
    pub async fn default_transport() -> Result<Self> {
        Self::udp4().await
    }

    #[allow(unused)]
    pub async fn unix() -> Result<Self> {
        Ok(Self::New {
            local: LocalEndpoint::UnixStream,
        })
    }

    #[allow(unused)]
    pub async fn tcp() -> Result<Self> {
        Ok(Self::New {
            local: LocalEndpoint::TcpStream,
        })
    }

    #[allow(unused)]
    #[inline]
    pub async fn udp4() -> Result<Self> {
        Self::udp(Domain::IPV4).await
    }

    #[allow(unused)]
    #[inline]
    pub async fn udp6() -> Result<Self> {
        Self::udp(Domain::IPV6).await
    }

    #[allow(unused)]
    async fn udp(domain: Domain) -> Result<Self> {
        let ip: IpAddr = match domain {
            Domain::IPV4 => Ipv4Addr::new(127, 0, 0, 1).into(),
            Domain::IPV6 => Ipv6Addr::from(1).into(),
            other => return Err(Error::Other(format!("Unknown socket domain: {other:?}"))),
        };

        let addr = SocketAddr::new(ip, 0);
        let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;

        socket.set_recv_buffer_size(SOCKET_BUFFER_SIZE)?;
        socket.set_send_buffer_size(SOCKET_BUFFER_SIZE)?;
        socket.set_nonblocking(true)?;

        socket.bind(&SockAddr::from(addr))?;

        let addr = socket
            .local_addr()?
            .as_socket()
            .ok_or_else(|| Error::Other("Unable to bind to {addr}".to_string()))?;

        log::info!("VM UDP endpoint bound at {addr}");

        let socket = UdpSocket::from_std(socket.into())?;
        Ok(Self::New {
            local: LocalEndpoint::UdpDatagram(Arc::new(socket)),
        })
    }

    pub async fn connect(&mut self, endpoint: impl Into<ContainerEndpoint>) -> Result<()> {
        match std::mem::replace(self, Self::Poisoned) {
            Self::New { local } => {
                let remote = match endpoint.into() {
                    ContainerEndpoint::UnixStream(path) => RemoteEndpoint::unix(path).await,
                    ContainerEndpoint::UdpDatagram(addr) => {
                        let s = match local {
                            LocalEndpoint::UdpDatagram(ref s) => s.clone(),
                            _ => return Err(Error::Other("Endpoint type mismatch".to_string())),
                        };

                        RemoteEndpoint::udp(addr, s).await
                    }
                    ContainerEndpoint::TcpStream(addr) => RemoteEndpoint::tcp(addr).await,
                    ep => Err(Error::Other(format!("Unsupported endpoint type: {:?}", ep))),
                }?;

                *self = Self::Connected { local, remote };
                Ok(())
            }
            e @ Self::Connected { .. } => {
                *self = e;
                Err(io::Error::from(io::ErrorKind::AlreadyExists).into())
            }
            Self::Poisoned => panic!("Programming error: endpoint in poisoned state"),
        }
    }

    pub fn local(&self) -> &LocalEndpoint {
        match self {
            Self::New { local } | Self::Connected { local, .. } => local,
            Self::Poisoned => panic!("Programming error: endpoint in poisoned state"),
        }
    }

    #[inline]
    pub fn send(&self, message: Result<Vec<u8>>) -> Result<()> {
        match self {
            Self::Connected { remote, .. } => remote.tx.send(message).map_err(Error::from),
            _ => Err(io::Error::from(io::ErrorKind::NotConnected).into()),
        }
    }

    #[inline]
    pub fn sender(&mut self) -> Result<mpsc::UnboundedSender<Result<Vec<u8>>>> {
        match self {
            Self::Connected { remote, .. } => Ok(remote.tx.clone()),
            _ => Err(io::Error::from(io::ErrorKind::NotConnected).into()),
        }
    }

    pub fn receiver(&mut self) -> Result<BoxStream<'static, Result<Vec<u8>>>> {
        match self {
            Self::Connected { remote, .. } => remote
                .rx
                .take()
                .ok_or_else(|| Error::Other("Endpoint already taken".to_string())),
            _ => Err(io::Error::from(io::ErrorKind::NotConnected).into()),
        }
    }
}

impl From<LocalEndpoint> for Endpoint {
    fn from(local: LocalEndpoint) -> Self {
        Self::New { local }
    }
}

#[non_exhaustive]
#[allow(unused)]
pub(crate) enum LocalEndpoint {
    UnixStream,
    UdpDatagram(Arc<UdpSocket>),
    TcpListener(TcpSocket),
    TcpStream,
}

impl std::fmt::Display for LocalEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnixStream => write!(f, "unix://"),
            Self::UdpDatagram(socket) => {
                let s = socket.local_addr().map_err(|_| std::fmt::Error {})?;
                write!(f, "udp://{}", s)
            }
            Self::TcpListener(socket) => {
                let s = socket.local_addr().map_err(|_| std::fmt::Error {})?;
                write!(f, "tcp-connect://{}", s)
            }
            Self::TcpStream => write!(f, "tcp-listen://"),
        }
    }
}

pub(crate) struct RemoteEndpoint {
    tx: mpsc::UnboundedSender<Result<Vec<u8>>>,
    rx: Option<BoxStream<'static, Result<Vec<u8>>>>,
}

impl RemoteEndpoint {
    // legacy socket endpoint (non-virtio)
    #[cfg(unix)]
    async fn unix<P: AsRef<Path>>(path: P) -> Result<Self> {
        const PREFIX_SIZE: usize = size_of::<u16>();
        const PREFIX_NE: bool = true;

        type SocketChannel = (
            mpsc::UnboundedSender<Result<Vec<u8>>>,
            mpsc::UnboundedReceiver<Result<Vec<u8>>>,
        );

        let path = path.as_ref();
        log::info!(
            "Connecting to Unix socket (stream) endpoint {}",
            path.display()
        );

        let socket = tokio::net::UnixStream::connect(path).await?;
        let (read, write) = io::split(socket);
        let (tx, rx): SocketChannel = mpsc::unbounded_channel();

        let stream = async_read_stream::<BUFFER_SIZE, _>(read, PREFIX_SIZE, PREFIX_NE);
        let writer = async_write_future(rx, write, PREFIX_SIZE, PREFIX_NE);

        tokio::task::spawn(writer);

        Ok(Self {
            tx,
            rx: Some(stream),
        })
    }

    #[cfg(not(unix))]
    async fn unix<P: AsRef<Path>>(_path: P) -> Result<Self> {
        Err(Error::Other("OS not supported".into()))
    }

    async fn tcp(addr: SocketAddr) -> Result<Self> {
        const PREFIX_SIZE: usize = size_of::<u32>();
        const PREFIX_NE: bool = false;

        type SocketChannel = (
            mpsc::UnboundedSender<Result<Vec<u8>>>,
            mpsc::UnboundedReceiver<Result<Vec<u8>>>,
        );

        let domain = if addr.is_ipv4() {
            Domain::IPV4
        } else {
            Domain::IPV6
        };

        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        socket.set_recv_buffer_size(SOCKET_BUFFER_SIZE)?;
        socket.set_send_buffer_size(SOCKET_BUFFER_SIZE)?;
        socket.set_nonblocking(true)?;

        log::info!("Connecting to TCP endpoint {addr}");
        socket.connect_timeout(&SockAddr::from(addr), Duration::from_secs(2))?;

        let stream = tokio::net::TcpStream::from_std(socket.into())?;
        let (read, write) = io::split(stream);
        let (tx, rx): SocketChannel = mpsc::unbounded_channel();

        log::info!("Spawning TCP endpoint event loop");
        let stream = async_read_stream::<BUFFER_SIZE, _>(read, PREFIX_SIZE, PREFIX_NE);
        let writer = async_write_future(rx, write, PREFIX_SIZE, PREFIX_NE);

        tokio::task::spawn(writer);

        Ok(Self {
            tx,
            rx: Some(stream),
        })
    }

    async fn udp(addr: SocketAddr, socket: Arc<UdpSocket>) -> Result<Self> {
        let read = socket.clone();
        let write = socket;
        let (tx, rx): SocketChannel = mpsc::unbounded_channel();

        let stream = {
            let buffer: [u8; BUFFER_SIZE] = [0u8; BUFFER_SIZE];
            futures::stream::unfold((read, buffer), |(r, mut b)| async move {
                match r.recv_from(&mut b).await.map(|t| t.0) {
                    Ok(0) => None,
                    Ok(n) => Some((Ok::<_, Error>(Vec::from(&b[..n])), (r, b))),
                    Err(e) => Some((Err(e.into()), (r, b))),
                }
            })
            .boxed()
        };

        log::info!("Spawning UDP endpoint event loop");
        let writer = async move {
            let mut rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
            loop {
                match StreamExt::next(&mut rx).await {
                    Some(Ok(data)) => {
                        if let Err(e) = write.send_to(data.as_slice(), addr).await {
                            log::error!("error writing to VM endpoint: {e}");
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        log::error!("VM endpoint error: {e}");
                        break;
                    }
                    None => break,
                }
            }
        }
        .boxed();
        tokio::task::spawn(writer);

        Ok(Self {
            tx,
            rx: Some(stream),
        })
    }
}

fn network_to_runtime_command(net: &DeploymentNetwork) -> Network {
    Network {
        addr: net.network.addr().to_string(),
        gateway: net.gateway.map(|g| g.to_string()).unwrap_or_default(),
        mask: net.network.netmask().to_string(),
        if_addr: net.node_ip.to_string(),
    }
}

fn async_read_stream<const N: usize, R>(
    read: R,
    prefix_size: usize,
    prefix_ne: bool,
) -> BoxStream<'static, Result<Vec<u8>>>
where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    let stream = {
        let buffer: [u8; N] = [0u8; N];
        futures::stream::unfold((read, buffer), |(mut r, mut b)| async move {
            match r.read(&mut b).await {
                Ok(0) => None,
                Ok(n) => Some((Ok::<_, io::Error>(BytesMut::from(&b[..n])), (r, b))),
                Err(e) => Some((Err(e), (r, b))),
            }
        })
    }
    .boxed();
    RxBufferStream::new(stream, N, prefix_size, prefix_ne).boxed()
}

fn async_write_future<'a, W>(
    rx: mpsc::UnboundedReceiver<Result<Vec<u8>>>,
    mut write: W,
    prefix_size: usize,
    prefix_ne: bool,
) -> BoxFuture<'a, ()>
where
    W: AsyncWriteExt + Unpin + Send + 'a,
{
    async move {
        let mut rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        loop {
            match StreamExt::next(&mut rx).await {
                Some(Ok(mut data)) => {
                    write_prefix(&mut data, prefix_size, prefix_ne);
                    if let Err(e) = write.write_all(data.as_slice()).await {
                        log::error!("VM endpoint write error: {e}");
                        break;
                    }
                }
                Some(Err(e)) => {
                    log::error!("VM endpoint error: {e}");
                    break;
                }
                None => break,
            }
        }
    }
    .boxed()
}

struct RxBuffer {
    inner: BytesMut,
    prefix_size: usize,
    prefix_ne: bool,
}

impl RxBuffer {
    pub fn new(capacity: usize, prefix_size: usize, prefix_ne: bool) -> Self {
        Self {
            inner: BytesMut::with_capacity(capacity),
            prefix_size,
            prefix_ne,
        }
    }

    pub fn process(&mut self, bytes: BytesMut) -> Option<Vec<u8>> {
        if self.inner.is_empty() {
            let _ = std::mem::replace(&mut self.inner, bytes);
        } else {
            self.inner.extend(bytes);
        }
        self.take_next()
    }

    fn read_prefix(&self) -> Option<usize> {
        match self.prefix_size {
            1 => prefix_u8::read(&self.inner, self.prefix_ne),
            2 => prefix_u16::read(&self.inner, self.prefix_ne),
            4 => prefix_u32::read(&self.inner, self.prefix_ne),
            8 => prefix_u64::read(&self.inner, self.prefix_ne),
            16 => prefix_u128::read(&self.inner, self.prefix_ne),
            _ => panic!("programming error: unsupported size: {}", self.prefix_size),
        }
    }

    fn has_next(&self) -> bool {
        self.read_prefix()
            .map(|n| self.inner.len() >= self.prefix_size + n)
            .unwrap_or(false)
    }

    fn take_next(&mut self) -> Option<Vec<u8>> {
        if let Some(n) = self.read_prefix() {
            if self.inner.len() >= self.prefix_size + n {
                self.inner.advance(self.prefix_size);
                return Some(self.inner.split_to(n).to_vec());
            }
        }
        None
    }
}

fn write_prefix(buf: &mut Vec<u8>, prefix_size: usize, prefix_ne: bool) {
    match prefix_size {
        1 => prefix_u8::write(buf, prefix_ne),
        2 => prefix_u16::write(buf, prefix_ne),
        4 => prefix_u32::write(buf, prefix_ne),
        8 => prefix_u64::write(buf, prefix_ne),
        16 => prefix_u128::write(buf, prefix_ne),
        _ => panic!("programming error: unsupported size: {}", prefix_size),
    }
}

struct RxBufferStream<S, E>
where
    S: Stream<Item = std::result::Result<BytesMut, E>>,
{
    rx_buffer: RxBuffer,
    inner: Peekable<S>,
}

impl<S, E> RxBufferStream<S, E>
where
    S: Stream<Item = std::result::Result<BytesMut, E>>,
{
    pub fn new(stream: S, capacity: usize, prefix_size: usize, prefix_ne: bool) -> Self {
        Self {
            rx_buffer: RxBuffer::new(capacity, prefix_size, prefix_ne),
            inner: stream.peekable(),
        }
    }
}

impl<S, E> Stream for RxBufferStream<S, E>
where
    S: Stream<Item = std::result::Result<BytesMut, E>> + Unpin + 'static,
    E: Into<Error> + 'static,
{
    type Item = Result<Vec<u8>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(vec) = self.rx_buffer.take_next() {
            if self.rx_buffer.has_next() {
                cx.waker().wake_by_ref();
            }
            return Poll::Ready(Some(Ok(vec)));
        }

        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                let mut wake_scheduled = false;
                if Pin::new(&mut self.inner).poll_peek(cx).is_ready() {
                    cx.waker().wake_by_ref();
                    wake_scheduled = true;
                }

                match self.rx_buffer.process(bytes) {
                    Some(vec) => {
                        if !wake_scheduled && self.rx_buffer.has_next() {
                            cx.waker().wake_by_ref();
                        }
                        Poll::Ready(Some(Ok(vec)))
                    }
                    _ => Poll::Pending,
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e.into()))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

fn gsb_endpoint(node_id: &str, net_id: &str) -> DuoEndpoint<GsbEndpoint> {
    DuoEndpoint {
        tcp: typed::service(format!("/net/{}/vpn/{}", node_id, net_id)),
        udp: typed::service(format!("/udp/net/{}/vpn/{}/raw", node_id, net_id)),
    }
}

macro_rules! impl_prefix {
    ($ty:ty, $m:ident) => {
        pub mod $m {
            use bytes::BytesMut;

            const SIZE: usize = std::mem::size_of::<$ty>();

            pub fn read(buf: &BytesMut, ne: bool) -> Option<usize> {
                if buf.len() < SIZE {
                    return None;
                }

                let mut sz: [u8; SIZE] = [0u8; SIZE];
                sz.copy_from_slice(&buf[..SIZE]);

                Some(if ne {
                    <$ty>::from_ne_bytes(sz)
                } else {
                    <$ty>::from_be_bytes(sz)
                } as usize)
            }

            #[inline]
            pub fn write(buf: &mut Vec<u8>, ne: bool) {
                let len = buf.len();
                buf.reserve(SIZE);
                if ne {
                    buf.splice(0..0, <$ty>::to_ne_bytes(len as $ty));
                } else {
                    buf.splice(0..0, <$ty>::to_be_bytes(len as $ty));
                }
            }
        }
    };
}

impl_prefix!(u8, prefix_u8);
impl_prefix!(u16, prefix_u16);
impl_prefix!(u32, prefix_u32);
impl_prefix!(u64, prefix_u64);
impl_prefix!(u128, prefix_u128);

#[cfg(test)]
mod test {
    use crate::network::RxBufferStream;
    use bytes::BytesMut;
    use futures::StreamExt;
    use std::iter::FromIterator;

    use super::write_prefix;

    const PREFIX_SIZE: usize = std::mem::size_of::<u32>();

    #[derive(Copy, Clone)]
    enum TxMode {
        Full,
        Chunked(usize),
    }

    impl TxMode {
        fn split(&self, v: Vec<u8>) -> Vec<Vec<u8>> {
            match self {
                Self::Full => vec![v],
                Self::Chunked(s) => v[..].chunks(*s).map(|c| c.to_vec()).collect(),
            }
        }
    }

    #[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
    struct TestError;

    impl From<TestError> for super::Error {
        fn from(_: TestError) -> Self {
            super::Error::Other("test error".to_string())
        }
    }

    async fn process_rx_buffer_stream(mode: TxMode, size: usize) {
        let src = (0..=255u8)
            .flat_map(|e| {
                let vec = Vec::from_iter(std::iter::repeat(e).take(size));
                mode.split(vec)
                    .into_iter()
                    .map(|mut v| {
                        write_prefix(&mut v, PREFIX_SIZE, false);
                        Ok::<_, TestError>(BytesMut::from_iter(v))
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let stream = futures::stream::iter(src.clone());
        let buf_stream = RxBufferStream::new(stream, 1500, PREFIX_SIZE, false);

        let dst = buf_stream
            .fold(vec![], |mut dst, item| async move {
                if let Ok(mut v) = item {
                    write_prefix(&mut v, PREFIX_SIZE, false);
                    dst.push(Ok::<_, TestError>(BytesMut::from_iter(v)));
                }
                dst
            })
            .await;

        assert_eq!(src, dst);
    }

    #[tokio::test]
    async fn rx_buffer_stream() {
        const PREFIX_SIZE: usize = 4;

        let modes = vec![TxMode::Full, TxMode::Chunked(1), TxMode::Chunked(2)];
        let sizes = [1, 2, 3, 5, 7, 12, 64];

        for mode in modes {
            for size in sizes {
                process_rx_buffer_stream(mode, size).await;
            }
        }
    }
}
