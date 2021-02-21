use crate::error::{Error, VpnError};
use crate::message::Shutdown;
use crate::state::Deployment;
use crate::Result;
use actix::prelude::*;
use futures::channel::mpsc;
use futures::{SinkExt, Stream, StreamExt, TryStreamExt};
use ipnet::IpNet;
use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::result::Result as StdResult;
use std::str::FromStr;
use tokio::io;
use ya_core_model::activity;
use ya_core_model::activity::VpnControl;
use ya_runtime_api::server::NetworkEndpoint;
use ya_service_bus::{actix_rpc, typed, typed::Endpoint, RpcEnvelope};

const DEFAULT_CHUNK_SIZE: usize = 65535;

pub fn gateway(net_addr: &str, net_mask: &str) -> Result<IpAddr> {
    let net = map_ip_net(net_addr, net_mask)?;
    net.hosts()
        .next()
        .ok_or_else(|| VpnError::NetAddrInvalid(net.addr()).into())
}

pub(crate) struct Vpn {
    activity_id: String,
    endpoint: VpnEndpoint,
    networks: HashMap<String, IpNet>,
    nodes: BTreeMap<Box<[u8]>, Endpoint>, // IP_BYTES_BE -> NODE_GSB_ADDR (for routing)
    nodes_rev: BTreeMap<String, Vec<Box<[u8]>>>, // NODE_ID -> Vec<IP_BYTES_BE> (for removal)
    packet_buf: PacketBuf,
}

impl Vpn {
    pub(crate) fn try_new(
        activity_id: String,
        endpoint: VpnEndpoint,
        deployment: Deployment,
    ) -> Result<Self> {
        let networks = deployment
            .networks
            .into_iter()
            .map(|net| Ok((net.id, map_ip_net(&net.ip, &net.mask)?)))
            .collect::<Result<_>>()?;

        log::info!("Registered VPN networks: {:?}", networks);

        let mut vpn = Self {
            activity_id,
            endpoint,
            networks,
            nodes: Default::default(),
            nodes_rev: Default::default(),
            packet_buf: Default::default(),
        };
        vpn.add(deployment.nodes)?;

        log::info!("Registered VPN nodes ({})", vpn.nodes.len());

        Ok(vpn)
    }

    #[allow(unused)]
    pub fn get<B: AsRef<[u8]>>(&self, addr: B) -> Option<&Endpoint> {
        self.nodes.get(addr.as_ref())
    }

    pub fn add<I, K>(&mut self, nodes: I) -> Result<()>
    where
        I: IntoIterator<Item = (K, String)>,
        K: AsRef<str>,
    {
        for result in map_ip_keys(nodes.into_iter()) {
            let (ip, node_id) = result?;
            let (net_id, _) = self
                .networks
                .iter()
                .find(|(_, net)| net.contains(&ip))
                .ok_or_else(|| VpnError::NetAddrInvalid(ip))?;
            let ip: Box<[u8]> = hton(ip).into();

            let endpoint = typed::service(format!("/net/{}/vpn/{}", node_id, net_id));
            let rev_entry = self.nodes_rev.entry(node_id).or_insert_with(Vec::new);
            rev_entry.push(ip.clone());
            self.nodes.insert(ip, endpoint);
        }

        Ok(())
    }

    pub fn remove<I, K>(&mut self, ids: I)
    where
        I: Iterator<Item = K>,
        K: AsRef<str>,
    {
        ids.for_each(|id| {
            self.nodes_rev.remove(id.as_ref()).map(|addrs| {
                addrs.into_iter().for_each(|a| {
                    self.nodes.remove(&a);
                });
            });
        });
    }

    fn handle_packet<'a>(
        &'a mut self,
        ip_off: usize,
        ip_len: usize,
        pkt: Vec<u8>,
        ctx: &'a mut Context<Self>,
    ) {
        let ip = &pkt[ip_off..ip_off + ip_len];
        log::trace!("Egress packet to {:?}", ip);

        if ip_len == 4 && &ip[0..4] == &[255, 255, 255, 255] {
            let fut_vec = self
                .nodes
                .values()
                .map(|e| e.call(activity::VpnPacket(pkt.clone())))
                .collect::<Vec<_>>();
            if !fut_vec.is_empty() {
                ctx.spawn(
                    async move {
                        let _ = futures::future::join_all(fut_vec).await;
                    }
                    .into_actor(self),
                );
            }
        } else {
            match self.nodes.get(ip) {
                Some(endpoint) => {
                    let fut = endpoint.call(activity::VpnPacket(pkt));
                    ctx.spawn(
                        async move {
                            if let Err(err) = fut.await {
                                log::debug!("Egress VPN call error: {}", err);
                            }
                        }
                        .into_actor(self),
                    );
                }
                None => log::trace!("No endpoint for {:?}", ip),
            }
        }
    }
}

impl Actor for Vpn {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();
        let srv_id = activity::exeunit::bus_id(&self.activity_id);
        actix_rpc::bind::<activity::VpnControl>(&srv_id, addr.clone().recipient());

        self.networks.keys().for_each(|net| {
            let vpn_id = activity::exeunit::network_id(net);
            actix_rpc::bind::<activity::VpnPacket>(&vpn_id, addr.clone().recipient());
        });

        let rx = match self.endpoint.rx.take() {
            Some(rx) => rx,
            None => {
                log::error!("No local VPN endpoint");
                return ctx.stop();
            }
        };

        Self::add_stream(rx, ctx);
        log::info!("Started VPN service");
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        log::info!("Stopping VPN service");

        let srv_id = activity::exeunit::bus_id(&self.activity_id);
        let networks = self.networks.keys().cloned().collect::<Vec<_>>();

        ctx.wait(
            async move {
                let _ = typed::unbind(&srv_id).await;
                for net in networks {
                    let vpn_id = activity::exeunit::network_id(&net);
                    let _ = typed::unbind(&vpn_id).await;
                }
            }
            .into_actor(self),
        );

        Running::Stop
    }
}

/// Egress traffic handler (VM -> VPN)
impl StreamHandler<Result<Vec<u8>>> for Vpn {
    fn handle(&mut self, result: Result<Vec<u8>>, ctx: &mut Context<Self>) {
        let mut data = match result {
            Ok(data) => data,
            Err(err) => return log::error!("Egress VPN error: {}", err),
        };
        let mut to_append = None;

        if let Ok(PeekResult::Packet {
            ip_off,
            ip_len,
            len,
        }) = peek_ip_pkt(&data[..])
        {
            self.packet_buf.clear();
            if data.len() > len {
                to_append.replace(data.split_off(len));
            }
            self.handle_packet(ip_off, ip_len, data, ctx);
        } else {
            to_append.replace(data);
        }

        if let Some(to_append) = to_append {
            let len = to_append.len();

            if self.packet_buf.capacity() < len {
                log::trace!("Received packet is too large: {}", len);
                return;
            } else if self.packet_buf.remaining() < len {
                log::trace!("Invalid packet read sequence from the network endpoint");
                self.packet_buf.clear();
                return;
            } else {
                self.packet_buf.append(&to_append);
            }
        }

        while let Ok(PeekResult::Packet {
            ip_off,
            ip_len,
            len,
        }) = peek_ip_pkt(&self.packet_buf.inner)
        {
            let data = self.packet_buf.take(len);
            self.handle_packet(ip_off, ip_len, data, ctx);
        }
    }
}

/// Ingress traffic handler (VPN -> VM)
impl Handler<RpcEnvelope<activity::VpnPacket>> for Vpn {
    type Result = <RpcEnvelope<activity::VpnPacket> as Message>::Result;

    fn handle(
        &mut self,
        packet: RpcEnvelope<activity::VpnPacket>,
        ctx: &mut Context<Self>,
    ) -> Self::Result {
        log::trace!("Ingress packet");

        let mut tx = self.endpoint.tx.clone();
        ctx.spawn(
            async move {
                if let Err(e) = tx.send(Ok(packet.into_inner().0)).await {
                    log::error!("Ingress VPN error: {}", e);
                }
            }
            .into_actor(self),
        );
        Ok(())
    }
}

impl Handler<RpcEnvelope<activity::VpnControl>> for Vpn {
    type Result = <RpcEnvelope<activity::VpnControl> as Message>::Result;

    fn handle(
        &mut self,
        msg: RpcEnvelope<activity::VpnControl>,
        _: &mut Context<Self>,
    ) -> Self::Result {
        match msg.into_inner() {
            VpnControl::UpdateNodes(nodes) => self.add(nodes)?,
            VpnControl::RemoveNodes(ids) => self.remove(ids.iter()),
        }
        Ok(())
    }
}

impl Handler<Shutdown> for Vpn {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, msg: Shutdown, ctx: &mut Context<Self>) -> Self::Result {
        log::info!("Shutting down VPN: {:?}", msg.0);
        ctx.stop();
        Ok(())
    }
}

pub struct VpnEndpoint {
    tx: mpsc::Sender<Result<Vec<u8>>>,
    rx: Option<Box<dyn Stream<Item = Result<Vec<u8>>> + Unpin>>,
}

impl VpnEndpoint {
    pub async fn connect(endpoint: NetworkEndpoint) -> Result<Self> {
        match endpoint {
            NetworkEndpoint::Socket(path) => Self::connect_socket(path).await,
        }
    }

    async fn connect_socket<P: AsRef<Path>>(path: P) -> Result<Self> {
        #[cfg(not(unix))]
        {
            unimplemented!();
        }
        #[cfg(unix)]
        {
            use bytes::Bytes;
            use tokio_util::codec::{BytesCodec, FramedRead, FramedWrite};

            let socket = tokio::net::UnixStream::connect(path.as_ref()).await?;
            let (read, write) = io::split(socket);

            let stream = FramedRead::with_capacity(read, BytesCodec::new(), DEFAULT_CHUNK_SIZE)
                .into_stream()
                .map_ok(|b| b.to_vec())
                .map_err(|e| Error::from(e));
            let sink = FramedWrite::new(write, BytesCodec::new())
                .with(|v| futures::future::ok(Bytes::from(v)));

            let (tx_si, rx_si) = mpsc::channel(1);
            Arbiter::spawn(async move {
                if let Err(e) = rx_si.forward(sink).await {
                    log::warn!("VPN socket sink forward error: {}", e);
                }
            });

            Ok(Self {
                tx: tx_si,
                rx: Some(Box::new(stream)),
            })
        }
    }
}

fn map_ip_net(ip: &str, mask: &str) -> StdResult<IpNet, VpnError> {
    let (ip, prefix_len) = match ip.find('/') {
        Some(idx) => {
            let ip_addr = IpAddr::from_str(&ip[..idx])?;
            let prefix_len = u32::from_str(&ip[idx + 1..])
                .map_err(|_| VpnError::IpAddrInvalidPrefix(ip.into()))?;
            (ip_addr, prefix_len)
        }
        None => match IpAddr::from_str(&ip)? {
            IpAddr::V4(ipv4) => {
                let mask = Ipv4Addr::from_str(&mask)?;
                let prefix_len = u32::from_ne_bytes(mask.octets()).to_be().leading_ones();
                (IpAddr::V4(ipv4), prefix_len)
            }
            IpAddr::V6(ipv6) => (IpAddr::V6(ipv6), 128),
        },
    };
    let net = IpNet::from_str(&format!("{}/{}", ip, prefix_len))
        .map_err(|_| VpnError::NetAddrInvalid(ip))?;
    Ok(net)
}

fn map_ip_keys<I, A, V>(addrs: I) -> impl Iterator<Item = Result<(IpAddr, V)>>
where
    I: IntoIterator<Item = (A, V)>,
    A: AsRef<str>,
{
    addrs.into_iter().map(|(addr, val)| {
        let ip = IpAddr::from_str(addr.as_ref()).map_err(VpnError::from)?;

        if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
            return Err(VpnError::IpAddrNotAllowed(ip).into());
        }
        // further checks require feature stabilization
        if let IpAddr::V4(ip4) = &ip {
            if ip4.is_broadcast() {
                return Err(VpnError::IpAddrNotAllowed(ip).into());
            }
        }

        Ok((ip, val))
    })
}

struct PacketBuf {
    inner: [u8; 2 * DEFAULT_CHUNK_SIZE],
    size: usize,
}

impl Default for PacketBuf {
    fn default() -> Self {
        Self {
            inner: [0u8; 2 * DEFAULT_CHUNK_SIZE],
            size: 0,
        }
    }
}

impl PacketBuf {
    #[inline(always)]
    fn append(&mut self, data: &[u8]) {
        let len = data.len();
        let inner = &mut self.inner[self.size..self.size + len];
        inner.copy_from_slice(data);
        self.size += len;
    }

    #[inline(always)]
    fn take(&mut self, len: usize) -> Vec<u8> {
        let len = len.min(self.size);
        let res = self.inner[..len].into();
        self.inner.rotate_left(len);
        self.size -= len;
        res
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.size = 0;
    }

    #[inline(always)]
    fn remaining(&self) -> usize {
        self.capacity() - self.size
    }

    #[inline(always)]
    fn capacity(&self) -> usize {
        self.inner.len()
    }
}

enum PeekResult {
    IncompleteHeader,
    IncompletePayload,
    Packet {
        ip_off: usize,
        ip_len: usize,
        len: usize,
    },
}

fn peek_ip_pkt(data: &[u8]) -> StdResult<PeekResult, VpnError> {
    match data.len() {
        0 => Ok(PeekResult::IncompleteHeader),
        _ => match data[0] >> 4 {
            4 => peek_ip4_pkt(data),
            6 => peek_ip6_pkt(data),
            _ => {
                Err(VpnError::PacketMalformed("Malformed IP header: invalid version".into()).into())
            }
        },
    }
}

fn peek_ip4_pkt(data: &[u8]) -> StdResult<PeekResult, VpnError> {
    if data.len() < 20 {
        return Ok(PeekResult::IncompleteHeader);
    }

    let total_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    if data.len() < total_len {
        return Ok(PeekResult::IncompletePayload);
    }

    Ok(PeekResult::Packet {
        ip_off: 16,
        ip_len: 4,
        len: total_len,
    })
}

fn peek_ip6_pkt(data: &[u8]) -> StdResult<PeekResult, VpnError> {
    if data.len() < 40 {
        return Ok(PeekResult::IncompleteHeader);
    }

    let payload_len = u16::from_be_bytes([data[4], data[5]]);
    let total_len = 40 + payload_len as usize;
    if payload_len == 0 {
        return Err(VpnError::PacketNotSupported("IPv6 jumbogram not supported".into()).into());
    } else if data.len() < total_len {
        return Ok(PeekResult::IncompletePayload);
    }

    Ok(PeekResult::Packet {
        ip_off: 24,
        ip_len: 16,
        len: total_len,
    })
}

#[inline(always)]
fn hton(ip: IpAddr) -> Box<[u8]> {
    match ip {
        IpAddr::V4(ip) => ip.octets().into(),
        IpAddr::V6(ip) => ip.octets().into(),
    }
}

#[allow(unused)]
#[inline(always)]
fn ntoh(data: &[u8]) -> Option<IpAddr> {
    if data.len() == 4 {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&data[0..4]);
        let ip = Ipv4Addr::from(u32::from_be_bytes(bytes));
        Some(IpAddr::V4(ip))
    } else if data.len() == 16 {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&data[0..16]);
        let ip = Ipv6Addr::from(bytes);
        Some(IpAddr::V6(ip))
    } else {
        None
    }
}
