use std::convert::TryFrom;
use std::path::Path;

use futures::Stream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use ya_runtime_api::deploy::ContainerEndpoint;
use ya_runtime_api::server::Network;
use ya_service_bus::{typed, typed::Endpoint as GsbEndpoint};
use ya_utils_networking::vpn::common::DEFAULT_MAX_FRAME_SIZE;
use ya_utils_networking::vpn::{network::DuoEndpoint, Error as NetError};

use crate::error::Error;
use crate::state::DeploymentNetwork;
use crate::Result;

pub(crate) mod inet;
pub(crate) mod vpn;

pub(crate) struct Endpoint {
    tx: mpsc::UnboundedSender<Result<Vec<u8>>>,
    rx: Option<Box<dyn Stream<Item = Result<Vec<u8>>> + Unpin>>,
}

impl Endpoint {
    pub async fn connect(endpoint: impl Into<ContainerEndpoint>) -> Result<Self> {
        match endpoint.into() {
            ContainerEndpoint::Socket(path) => Self::connect_to_socket(path).await,
            ep => Err(Error::Other(format!("Unsupported endpoint type: {:?}", ep))),
        }
    }

    #[cfg(unix)]
    async fn connect_to_socket<P: AsRef<Path>>(path: P) -> Result<Self> {
        use futures::StreamExt;
        use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

        type SocketChannel = (
            UnboundedSender<Result<Vec<u8>>>,
            UnboundedReceiver<Result<Vec<u8>>>,
        );

        let socket = tokio::net::UnixStream::connect(path.as_ref()).await?;
        let (read, mut write) = tokio::io::split(socket);
        let (tx_si, rx_si): SocketChannel = mpsc::unbounded_channel();

        let stream = {
            let buffer: [u8; DEFAULT_MAX_FRAME_SIZE] = [0u8; DEFAULT_MAX_FRAME_SIZE];
            futures::stream::unfold((read, buffer), |(mut r, mut b)| async move {
                match r.read(&mut b).await {
                    Ok(0) => None,
                    Ok(n) => Some((Ok(b[..n].to_vec()), (r, b))),
                    Err(e) => Some((Err(e.into()), (r, b))),
                }
            })
            .boxed_local()
        };

        tokio::task::spawn_local(async move {
            let mut rx_si = UnboundedReceiverStream::new(rx_si);
            loop {
                match StreamExt::next(&mut rx_si).await {
                    Some(Ok(data)) => {
                        if let Err(e) = write.write_all(data.as_slice()).await {
                            log::error!("error writing to VM socket endpoint: {e}");
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        log::error!("VM socket endpoint error: {e}");
                        break;
                    }
                    None => break,
                }
            }
        });

        Ok(Self {
            tx: tx_si,
            rx: Some(Box::new(stream)),
        })
    }

    #[cfg(not(unix))]
    async fn connect_to_socket<P: AsRef<Path>>(_path: P) -> Result<Self> {
        Err(Error::Other("OS not supported".into()))
    }
}

impl<'a> TryFrom<&'a DeploymentNetwork> for Network {
    type Error = Error;

    fn try_from(net: &'a DeploymentNetwork) -> Result<Self> {
        let ip = net.network.addr();
        let mask = net.network.netmask();
        let gateway = net
            .network
            .hosts()
            .find(|ip_| ip_ != &ip)
            .ok_or(NetError::NetAddrTaken(ip))?;

        Ok(Network {
            addr: ip.to_string(),
            gateway: gateway.to_string(),
            mask: mask.to_string(),
            if_addr: net.node_ip.to_string(),
        })
    }
}

type Prefix = u16;
const PREFIX_SIZE: usize = std::mem::size_of::<Prefix>();

pub(self) struct RxBuffer {
    expected: usize,
    inner: Vec<u8>,
}

impl Default for RxBuffer {
    fn default() -> Self {
        Self {
            expected: 0,
            inner: Vec::with_capacity(PREFIX_SIZE + DEFAULT_MAX_FRAME_SIZE),
        }
    }
}

impl RxBuffer {
    pub fn process(&mut self, received: Vec<u8>) -> RxIterator {
        RxIterator {
            buffer: self,
            received,
        }
    }
}

struct RxIterator<'a> {
    buffer: &'a mut RxBuffer,
    received: Vec<u8>,
}

impl<'a> Iterator for RxIterator<'a> {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer.expected > 0 && !self.received.is_empty() {
            let len = self.buffer.expected.min(self.received.len());
            self.buffer.inner.extend(self.received.drain(..len));
        }

        if let Some(len) = read_prefix(&self.buffer.inner) {
            if let Some(item) = take_next(&mut self.buffer.inner, len) {
                self.buffer.expected = read_prefix(&self.buffer.inner).unwrap_or(0) as usize;
                return Some(item);
            }
        }

        if let Some(len) = read_prefix(&self.received) {
            if let Some(item) = take_next(&mut self.received, len) {
                return Some(item);
            }
        }

        self.buffer.inner.append(&mut self.received);
        if let Some(len) = read_prefix(&self.buffer.inner) {
            self.buffer.expected = len as usize;
        }

        None
    }
}

fn take_next(src: &mut Vec<u8>, len: Prefix) -> Option<Vec<u8>> {
    let p_len = PREFIX_SIZE + len as usize;
    if src.len() >= p_len {
        return Some(src.drain(..p_len).skip(PREFIX_SIZE).collect());
    }
    None
}

fn read_prefix(src: &[u8]) -> Option<Prefix> {
    if src.len() < PREFIX_SIZE {
        return None;
    }
    let mut u16_buf = [0u8; PREFIX_SIZE];
    u16_buf.copy_from_slice(&src[..PREFIX_SIZE]);
    Some(u16::from_ne_bytes(u16_buf))
}

fn write_prefix(dst: &mut Vec<u8>) {
    let len_u16 = dst.len() as u16;
    dst.reserve(PREFIX_SIZE);
    dst.splice(0..0, u16::to_ne_bytes(len_u16).to_vec());
}

fn gsb_endpoint(node_id: &str, net_id: &str) -> DuoEndpoint<GsbEndpoint> {
    DuoEndpoint {
        tcp: typed::service(format!("/net/{}/vpn/{}", node_id, net_id)),
        udp: typed::service(format!("/udp/net/{}/vpn/{}/raw", node_id, net_id)),
    }
}

#[cfg(test)]
mod test {
    use std::iter::FromIterator;

    use super::{write_prefix, RxBuffer};

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

    #[test]
    fn rx_buffer() {
        for tx in vec![TxMode::Full, TxMode::Chunked(1), TxMode::Chunked(2)] {
            for sz in [1, 2, 3, 5, 7, 12, 64] {
                let src = (0..=255u8)
                    .into_iter()
                    .map(|e| Vec::from_iter(std::iter::repeat(e).take(sz)))
                    .collect::<Vec<_>>();

                let mut buf = RxBuffer::default();
                let mut dst = Vec::with_capacity(src.len());

                src.iter().cloned().for_each(|mut v| {
                    write_prefix(&mut v);
                    for received in tx.split(v) {
                        for item in buf.process(received) {
                            dst.push(item);
                        }
                    }
                });

                assert_eq!(src, dst);
            }
        }
    }
}
