#![allow(unused)]
use crate::error::Error;
use crate::message::Shutdown;
use crate::state::{Deployment, DeploymentNetwork};
use crate::Result;
use actix::prelude::*;
use futures::channel::mpsc;
use futures::future;
use futures::{FutureExt, SinkExt, Stream, StreamExt, TryFutureExt, TryStreamExt};
use std::convert::TryFrom;
use std::ops::Not;
use std::path::Path;
use tokio::io;

use crate::acl::Acl;
use ya_core_model::activity;
use ya_core_model::activity::{RpcMessageError, VpnControl};
use ya_runtime_api::server::{CreateNetwork, Network, NetworkEndpoint, RuntimeService};
use ya_service_bus::{actix_rpc, typed, typed::Endpoint as GsbEndpoint, RpcEnvelope};
use ya_utils_networking::vpn::common::ntoh;
use ya_utils_networking::vpn::error::Error as VpnError;
use ya_utils_networking::vpn::{
    self, ArpField, ArpPacket, EtherFrame, EtherType, IpPacket, Networks, PeekPacket,
    MAX_FRAME_SIZE,
};

pub(crate) async fn start_vpn<R: RuntimeService>(
    acl: Acl,
    service: &R,
    deployment: Deployment,
) -> Result<Option<Addr<Vpn>>> {
    if !deployment.networking() {
        return Ok(None);
    }

    let networks = deployment
        .networks
        .values()
        .map(TryFrom::try_from)
        .collect::<Result<_>>()?;
    let hosts = deployment.hosts.clone();
    let response = service
        .create_network(CreateNetwork { networks, hosts })
        .await
        .map_err(|e| Error::Other(format!("Network setup error: {:?}", e)))?;

    let endpoint = match response.endpoint {
        Some(endpoint) => VpnEndpoint::connect(endpoint).await?,
        None => return Err(Error::Other("VPN endpoint already connected".into()).into()),
    };

    let vpn = Vpn::try_new(acl, endpoint, deployment)?;
    Ok(Some(vpn.start()))
}

pub(crate) struct Vpn {
    // TODO: Populate & use ACL
    acl: Acl,
    networks: Networks<GsbEndpoint>,
    endpoint: VpnEndpoint,
    rx_buf: Option<RxBuffer>,
}

impl Vpn {
    fn try_new(acl: Acl, endpoint: VpnEndpoint, deployment: Deployment) -> Result<Self> {
        let mut networks = vpn::Networks::default();

        deployment
            .networks
            .iter()
            .try_for_each(|(id, net)| networks.add(id.clone(), net.network))?;

        deployment.networks.into_iter().try_for_each(|(id, net)| {
            let network = networks.get_mut(&id).unwrap();
            net.nodes
                .into_iter()
                .try_for_each(|(ip, id)| network.add_node(ip, &id, gsb_endpoint))?;
            Ok::<_, VpnError>(())
        })?;

        Ok(Self {
            acl,
            networks,
            endpoint,
            rx_buf: Some(Default::default()),
        })
    }

    fn handle_ip(&mut self, frame: EtherFrame, ctx: &mut Context<Self>) {
        let ip_pkt = IpPacket::packet(frame.payload());
        log::trace!("Egress packet to {:?}", ip_pkt.dst_address());

        if ip_pkt.is_broadcast() {
            let futs = self
                .networks
                .endpoints()
                .into_iter()
                .map(|e| e.call(activity::VpnPacket(frame.as_ref().to_vec())))
                .collect::<Vec<_>>();
            futs.is_empty().not().then(|| {
                let fut = future::join_all(futs).then(|_| future::ready(()));
                ctx.spawn(fut.into_actor(self))
            });
        } else {
            let ip = ip_pkt.dst_address();
            match self.networks.endpoint(ip) {
                Some(endpoint) => self.forward_frame(endpoint, frame, ctx),
                None => log::debug!("No endpoint for {:?}", ip),
            }
        }
    }

    fn handle_arp(&mut self, frame: EtherFrame, ctx: &mut Context<Self>) {
        let arp = ArpPacket::packet(frame.payload());
        // forward only IP ARP packets
        if arp.get_field(ArpField::PTYPE) != &[08, 00] {
            return;
        }

        let ip = arp.get_field(ArpField::TPA);
        match self.networks.endpoint(ip) {
            Some(endpoint) => self.forward_frame(endpoint, frame, ctx),
            None => log::debug!("No endpoint for {:?}", ip),
        }
    }

    fn forward_frame(&mut self, endpoint: GsbEndpoint, frame: EtherFrame, ctx: &mut Context<Self>) {
        let pkt: Vec<_> = frame.into();
        log::trace!("Egress {} b", pkt.len());

        endpoint
            .call(activity::VpnPacket(pkt))
            .map_err(|err| log::debug!("VPN call error: {}", err))
            .then(|_| future::ready(()))
            .into_actor(self)
            .spawn(ctx);
    }
}

impl Actor for Vpn {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.networks.as_ref().keys().for_each(|net| {
            let actor = ctx.address();
            let net_id = net.clone();
            let vpn_id = activity::exeunit::network_id(&net_id);

            actix_rpc::bind::<activity::VpnControl>(&vpn_id, ctx.address().recipient());
            typed::bind_with_caller::<activity::VpnPacket, _, _>(&vpn_id, move |caller, pkt| {
                actor
                    .send(Packet {
                        network_id: net_id.clone(),
                        caller,
                        data: pkt.0,
                    })
                    .then(|sent| match sent {
                        Ok(result) => future::ready(result),
                        Err(err) => future::err(RpcMessageError::Service(err.to_string())),
                    })
            });
        });

        match self.endpoint.rx.take() {
            Some(rx) => {
                Self::add_stream(rx, ctx);
                log::info!("Started VPN service")
            }
            None => {
                ctx.stop();
                log::error!("No local VPN endpoint");
            }
        };
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        log::info!("Stopping VPN service");

        let networks = self.networks.as_ref().keys().cloned().collect::<Vec<_>>();
        async move {
            for net in networks {
                let vpn_id = activity::exeunit::network_id(&net);
                let _ = typed::unbind(&vpn_id).await;
            }
        }
        .into_actor(self)
        .wait(ctx);

        Running::Stop
    }
}

/// Egress traffic handler (VM -> VPN)
impl StreamHandler<Result<Vec<u8>>> for Vpn {
    fn handle(&mut self, result: Result<Vec<u8>>, ctx: &mut Context<Self>) {
        let received = match result {
            Ok(vec) => vec,
            Err(err) => return log::debug!("VPN error (egress): {}", err),
        };
        let mut rx_buf = match self.rx_buf.take() {
            Some(buf) => buf,
            None => return log::error!("Programming error: rx buffer already taken"),
        };

        for packet in rx_buf.process(received) {
            let frame = match EtherFrame::try_from(packet) {
                Ok(frame) => match &frame {
                    EtherFrame::Arp(_) => self.handle_arp(frame, ctx),
                    EtherFrame::Ip(_) => self.handle_ip(frame, ctx),
                    frame => log::debug!("VPN: unimplemented EtherType: {}", frame),
                },
                Err(err) => {
                    match &err {
                        VpnError::ProtocolNotSupported(_) => (),
                        _ => log::debug!("VPN frame error (egress): {}", err),
                    };
                    continue;
                }
            };
        }

        self.rx_buf.replace(rx_buf);
    }
}

/// Ingress traffic handler (VPN -> VM)
impl Handler<Packet> for Vpn {
    type Result = <Packet as Message>::Result;

    fn handle(&mut self, mut packet: Packet, ctx: &mut Context<Self>) -> Self::Result {
        log::trace!("Ingress {} b", packet.data.len());

        let network_id = packet.network_id;
        let node_id = packet.caller;
        let data = packet.data.into_boxed_slice();

        // fixme: should requestor be queried for unknown IP addresses instead?
        // read and add unknown node id -> ip if it doesn't exist
        if let Ok(ether_type) = EtherFrame::peek_type(&data) {
            let payload = EtherFrame::peek_payload(&data).unwrap();
            let ip = match ether_type {
                EtherType::Arp => {
                    let pkt = ArpPacket::packet(payload);
                    ntoh(pkt.get_field(ArpField::SPA))
                }
                EtherType::Ip => {
                    let pkt = IpPacket::packet(payload);
                    ntoh(pkt.src_address())
                }
                _ => None,
            };

            if let Some(ip) = ip {
                self.networks.get_mut(&network_id).map(|network| {
                    if !network.nodes().contains_key(&node_id) {
                        log::debug!("Adding new node: {} {}", ip, node_id);
                        network.add_node(ip, &node_id, gsb_endpoint);
                    }
                });
            }
        }

        let mut data = data.into();
        write_prefix(&mut data);

        let mut tx = self.endpoint.tx.clone();
        async move {
            if let Err(e) = tx.send(Ok(data)).await {
                log::debug!("Ingress VPN error: {}", e);
            }
        }
        .into_actor(self)
        .spawn(ctx);

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
        // if !self.acl.has_access(msg.caller(), AccessRole::Control) {
        //     return Err(AclError::Forbidden(msg.caller().to_string(), AccessRole::Control).into());
        // }

        match msg.into_inner() {
            VpnControl::AddNodes { network_id, nodes } => {
                let network = self.networks.get_mut(&network_id).map_err(Error::from)?;
                for (ip, id) in Deployment::map_nodes(nodes).map_err(Error::from)? {
                    network
                        .add_node(ip, &id, gsb_endpoint)
                        .map_err(Error::from)?;
                }
            }
            VpnControl::RemoveNodes {
                network_id,
                node_ids,
            } => {
                let network = self.networks.get_mut(&network_id).map_err(Error::from)?;
                node_ids.into_iter().for_each(|id| network.remove_node(&id));
            }
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

#[derive(Message)]
#[rtype(result = "<RpcEnvelope<activity::VpnPacket> as Message>::Result")]
struct Packet {
    network_id: String,
    caller: String,
    data: Vec<u8>,
}

struct VpnEndpoint {
    tx: mpsc::Sender<Result<Vec<u8>>>,
    rx: Option<Box<dyn Stream<Item = Result<Vec<u8>>> + Unpin>>,
}

impl VpnEndpoint {
    pub async fn connect(endpoint: NetworkEndpoint) -> Result<Self> {
        match endpoint {
            NetworkEndpoint::Socket(path) => Self::connect_socket(path).await,
        }
    }

    #[cfg(unix)]
    async fn connect_socket<P: AsRef<Path>>(path: P) -> Result<Self> {
        use bytes::Bytes;
        use tokio_util::codec::{BytesCodec, FramedRead, FramedWrite};

        let socket = tokio::net::UnixStream::connect(path.as_ref()).await?;
        let (read, write) = io::split(socket);

        let sink = FramedWrite::new(write, BytesCodec::new()).with(|v| future::ok(Bytes::from(v)));
        let stream = FramedRead::with_capacity(read, BytesCodec::new(), MAX_FRAME_SIZE)
            .into_stream()
            .map_ok(|b| b.to_vec())
            .map_err(|e| Error::from(e));

        let (tx_si, rx_si) = mpsc::channel(1);
        Arbiter::spawn(async move {
            if let Err(e) = rx_si.forward(sink).await {
                log::error!("VPN socket forward error: {}", e);
            }
        });

        Ok(Self {
            tx: tx_si,
            rx: Some(Box::new(stream)),
        })
    }

    #[cfg(not(unix))]
    async fn connect_socket<P: AsRef<Path>>(path: P) -> Result<Self> {
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
            .ok_or_else(|| VpnError::NetAddrTaken(ip))?;

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
            inner: Vec::with_capacity(PREFIX_SIZE + MAX_FRAME_SIZE),
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
        if self.buffer.expected > 0 && self.received.len() > 0 {
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

        self.buffer.inner.extend(self.received.drain(..));
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

fn read_prefix(src: &Vec<u8>) -> Option<Prefix> {
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

fn gsb_endpoint(node_id: &str, net_id: &str) -> GsbEndpoint {
    typed::service(format!("/net/{}/vpn/{}", node_id, net_id))
}

#[cfg(test)]
mod test {
    use super::{write_prefix, RxBuffer};
    use std::iter::FromIterator;

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
        for mut tx in vec![TxMode::Full, TxMode::Chunked(1), TxMode::Chunked(2)] {
            for sz in vec![1, 2, 3, 5, 7, 12, 64] {
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
