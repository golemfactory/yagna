use std::collections::{BTreeSet, HashMap};
use std::convert::TryFrom;
use std::net::IpAddr;
use std::ops::DerefMut;
use std::str::FromStr;
use std::time::Duration;

use actix::prelude::*;
use actix_web::error::Canceled;
use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::{future, TryFutureExt};
use futures::{FutureExt, SinkExt};
use smoltcp::iface::Route;
use smoltcp::socket::{Socket, SocketHandle};
use smoltcp::wire::{IpAddress, IpCidr, IpEndpoint};
use uuid::Uuid;

use crate::message::*;
use crate::socket::*;
use crate::stack::Stack;
use crate::Result;

use ya_core_model::activity::{VpnControl, VpnPacket};
use ya_core_model::NodeId;
use ya_service_bus::typed::{self, Endpoint};
use ya_service_bus::{actix_rpc, RpcEndpoint, RpcEnvelope};
use ya_utils_networking::vpn::common::{to_ip, to_net};
use ya_utils_networking::vpn::*;

const STACK_POLL_INTERVAL: Duration = Duration::from_millis(2500);

#[derive(Default)]
pub struct VpnSupervisor {
    networks: HashMap<String, Addr<Vpn>>,
    blueprints: HashMap<String, ya_client_model::net::Network>,
    ownership: HashMap<NodeId, BTreeSet<String>>,
    arbiter: Arbiter,
}

impl VpnSupervisor {
    pub fn get_networks(&self, node_id: &NodeId) -> Vec<ya_client_model::net::Network> {
        self.ownership
            .get(node_id)
            .map(|networks| {
                networks
                    .iter()
                    .filter_map(|id| self.blueprints.get(id.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_else(Vec::new)
    }

    pub fn get_network(&self, node_id: &NodeId, network_id: &str) -> Result<Addr<Vpn>> {
        self.owner(node_id, network_id)?;
        self.vpn(network_id)
    }

    pub fn get_blueprint(
        &self,
        node_id: &NodeId,
        network_id: &str,
    ) -> Result<ya_client_model::net::Network> {
        self.owner(node_id, network_id)?;
        self.blueprints
            .get(network_id)
            .cloned()
            .ok_or_else(|| Error::NetNotFound)
    }

    pub async fn create_network(
        &mut self,
        node_id: &NodeId,
        network: ya_client_model::net::NewNetwork,
    ) -> Result<ya_client_model::net::Network> {
        let net = to_net(&network.ip, network.mask.as_ref())?;
        let net_id = Uuid::new_v4().to_simple().to_string();
        let net_ip = IpCidr::new(net.addr().into(), net.prefix_len());
        let net_gw = match network
            .gateway
            .as_ref()
            .map(|g| IpAddr::from_str(&g))
            .transpose()?
        {
            Some(gw) => gw,
            None => net
                .hosts()
                .next()
                .ok_or_else(|| Error::NetCidr(net.addr(), net.prefix_len()))?,
        };

        let vpn_net = Network::new(&net_id, net);
        let actor = self
            .arbiter
            .clone()
            .spawn_ext(async move {
                let stack = Stack::new(net_ip, net_route(net_gw.clone())?);
                let vpn = Vpn::new(stack, vpn_net);
                Ok::<_, Error>(vpn.start())
            })
            .await?;

        let network = ya_client_model::net::Network {
            id: net_id.clone(),
            ip: net_ip.to_string(),
            mask: net.netmask().to_string(),
            gateway: net_gw.to_string(),
        };

        self.networks.insert(net_id.clone(), actor);
        self.blueprints.insert(net_id.clone(), network.clone());
        self.ownership
            .entry(node_id.clone())
            .or_insert_with(Default::default)
            .insert(net_id);

        Ok(network)
    }

    pub fn remove_network<'a>(
        &mut self,
        node_id: &NodeId,
        network_id: &str,
    ) -> Result<BoxFuture<'a, Result<()>>> {
        self.owner(node_id, network_id)?;
        let vpn = self
            .networks
            .remove(network_id)
            .ok_or_else(|| Error::NetNotFound)?;
        self.blueprints.remove(network_id);
        self.forward(vpn, Shutdown {})
    }

    pub fn remove_node<'a>(
        &mut self,
        node_id: &NodeId,
        network_id: &str,
        id: String,
    ) -> Result<BoxFuture<'a, Result<()>>> {
        self.owner(node_id, network_id)?;
        self.ownership.remove(node_id);
        let vpn = self.vpn(network_id)?;
        self.forward(vpn, RemoveNode { id })
    }

    fn forward<'a, M, T>(
        &self,
        vpn: Addr<Vpn>,
        msg: M,
    ) -> Result<BoxFuture<'a, <M as Message>::Result>>
    where
        Vpn: Handler<M>,
        M: Message<Result = std::result::Result<T, Error>> + Send + 'static,
        <M as Message>::Result: Send + 'static,
        T: Send + 'static,
    {
        Ok(Box::pin(async move {
            match vpn.send(msg).await {
                Ok(r) => r,
                Err(_) => Err(Error::NetNotFound),
            }
        }))
    }

    fn vpn(&self, network_id: &str) -> Result<Addr<Vpn>> {
        self.networks
            .get(network_id)
            .cloned()
            .ok_or_else(|| Error::NetNotFound)
    }

    fn owner(&self, node_id: &NodeId, network_id: &str) -> Result<()> {
        self.ownership
            .get(node_id)
            .map(|s| s.contains(network_id))
            .ok_or_else(|| Error::NetNotFound)?
            .then(|| ())
            .ok_or_else(|| Error::Forbidden)
    }
}

pub struct Vpn {
    vpn: Network<Endpoint>,
    stack: Stack<'static>,
    connections: HashMap<SocketHandle, Connection>,
}

impl Vpn {
    pub fn new(stack: Stack<'static>, vpn: Network<Endpoint>) -> Self {
        Self {
            vpn,
            stack,
            connections: Default::default(),
        }
    }

    fn poll(&mut self, addr: Addr<Self>) {
        loop {
            if let Err(err) = self.stack.poll() {
                log::warn!("VPN {}: socket poll error: {}", self.vpn.id(), err);
            }

            let egress = self.process_egress();
            let ingress = self.process_ingress(addr.clone());

            if !egress && !ingress {
                break;
            }
        }
    }

    fn process_ingress(&mut self, addr: Addr<Self>) -> bool {
        let mut processed = false;

        let id = self.vpn.id().clone();
        let connections = &self.connections;
        let socket_rfc = self.stack.sockets();
        let mut sockets = socket_rfc.borrow_mut();

        for mut socket_ref in (*sockets).iter_mut() {
            let socket: &mut Socket = socket_ref.deref_mut();
            let handle = socket.handle();

            if !socket.is_open() {
                addr.do_send(Disconnect::new(handle, DisconnectReason::SocketClosed));
                continue;
            }

            while socket.can_recv() {
                let (remote, data) = match socket.recv() {
                    Ok(Some(tup)) => {
                        processed = true;
                        tup
                    }
                    Ok(None) => break,
                    Err(err) => {
                        log::warn!("VPN {}: packet error: {}", id, err);
                        processed = true;
                        continue;
                    }
                };

                let mut user_tx = match connections.get(&handle) {
                    Some(conn) => conn.tx.clone(),
                    None => {
                        log::warn!("VPN {}: no connection to {:?}", id, remote);
                        continue;
                    }
                };

                let addr_ = addr.clone();
                tokio::task::spawn_local(async move {
                    if let Err(_) = user_tx.send(data).await {
                        addr_.do_send(Disconnect::new(handle, DisconnectReason::SinkClosed));
                    }
                });
            }
        }

        processed
    }

    fn process_egress<'a>(&mut self) -> bool {
        let mut processed = false;
        let vpn_id = self.vpn.id().clone();

        let iface_rfc = self.stack.iface();
        let mut iface = iface_rfc.borrow_mut();
        let device = iface.device_mut();

        while let Some(data) = device.next_phy_tx() {
            processed = true;

            let frame = match EtherFrame::try_from(data) {
                Ok(frame) => frame,
                Err(err) => {
                    log::error!("VPN {}: Ethernet frame error: {}", vpn_id, err);
                    continue;
                }
            };

            let endpoint = match &frame {
                EtherFrame::Ip(_) => {
                    let packet = IpPacket::packet(frame.payload());
                    log::trace!("Egress IP packet to {:?}", packet.dst_address());
                    self.vpn.endpoint(packet.dst_address())
                }
                EtherFrame::Arp(_) => {
                    let packet = ArpPacket::packet(frame.payload());
                    log::trace!("Egress ARP packet to {:?}", packet.get_field(ArpField::TPA));
                    self.vpn.endpoint(packet.get_field(ArpField::TPA))
                }
                _ => {
                    log::error!("VPN {}: unimplemented Ethernet frame type", vpn_id);
                    continue;
                }
            };
            let endpoint = match endpoint {
                Some(endpoint) => endpoint,
                None => {
                    log::trace!("No endpoint for egress packet");
                    continue;
                }
            };

            let id = vpn_id.clone();
            tokio::task::spawn_local(async move {
                if let Err(err) = endpoint.send(VpnPacket(frame.into())).await {
                    let addr = endpoint.addr();
                    log::warn!("VPN {}: send error to endpoint '{}': {}", id, addr, err);
                }
            });
        }

        processed
    }
}

impl Actor for Vpn {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let id = self.vpn.id();
        let vpn_url = gsb_local_url(&id);
        actix_rpc::bind::<VpnPacket>(&vpn_url, ctx.address().recipient());

        ctx.run_interval(STACK_POLL_INTERVAL, |this, ctx| {
            this.poll(ctx.address());
        });

        log::info!("VPN {} started", id);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        log::warn!("Stopping VPN {}", self.vpn.id());
        Running::Stop
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        let id = self.vpn.id().clone();
        let vpn_url = gsb_local_url(&id);

        async move {
            let _ = typed::unbind(&vpn_url).await;
            log::info!("VPN {} stopped", id);
        }
        .into_actor(self)
        .wait(ctx);
    }
}

impl Handler<GetAddresses> for Vpn {
    type Result = <GetAddresses as Message>::Result;

    fn handle(&mut self, _: GetAddresses, _: &mut Self::Context) -> Self::Result {
        Ok(self
            .stack
            .addresses()
            .into_iter()
            .map(|ip| ya_client_model::net::Address { ip: ip.to_string() })
            .collect())
    }
}

impl Handler<AddAddress> for Vpn {
    type Result = <AddAddress as Message>::Result;

    fn handle(&mut self, msg: AddAddress, _: &mut Self::Context) -> Self::Result {
        let ip: IpAddr = msg.address.parse()?;

        let net = self.vpn.as_ref();
        if !net.contains(&ip) {
            return Err(Error::NetAddrMismatch(ip));
        }

        let cidr = IpCidr::new(IpAddress::from(ip), net.prefix_len());
        if !cidr.address().is_unicast() && !cidr.address().is_unspecified() {
            return Err(Error::IpAddrNotAllowed(ip));
        }

        self.stack.add_address(cidr);
        self.vpn.add_address(&msg.address)?;

        Ok(())
    }
}

impl Handler<GetNodes> for Vpn {
    type Result = <GetNodes as Message>::Result;

    fn handle(&mut self, _: GetNodes, _: &mut Self::Context) -> Self::Result {
        Ok(self
            .vpn
            .nodes()
            .iter()
            .map(|(id, ips)| {
                ips.iter()
                    .map(|ip| ya_client_model::net::Node {
                        id: id.clone(),
                        ip: ip.to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .flatten()
            .collect())
    }
}

impl Handler<AddNode> for Vpn {
    type Result = <AddNode as Message>::Result;

    fn handle(&mut self, msg: AddNode, _: &mut Self::Context) -> Self::Result {
        let ip = to_ip(&msg.address)?;
        match self.vpn.add_node(ip, &msg.id, gsb_remote_url) {
            Ok(_) | Err(Error::IpAddrTaken(_)) => {}
            Err(err) => return Err(err),
        }

        let vpn_id = self.vpn.id().clone();
        let futs = self
            .vpn
            .endpoints()
            .values()
            .cloned()
            .map(|e| {
                e.send(VpnControl::AddNodes {
                    network_id: vpn_id.clone(),
                    nodes: vec![(msg.address.clone(), msg.id.clone())]
                        .into_iter()
                        .collect(),
                })
            })
            .collect::<Vec<_>>();

        tokio::task::spawn_local(async move {
            let _ = future::join_all(futs).await;
        });

        Ok(())
    }
}

impl Handler<RemoveNode> for Vpn {
    type Result = <RemoveNode as Message>::Result;

    fn handle(&mut self, msg: RemoveNode, _: &mut Self::Context) -> Self::Result {
        self.vpn.remove_node(&msg.id);

        let vpn_id = self.vpn.id().clone();
        let futs = self
            .vpn
            .endpoints()
            .values()
            .cloned()
            .map(|e| {
                e.send(VpnControl::RemoveNodes {
                    network_id: vpn_id.clone(),
                    node_ids: vec![msg.id.clone()].into_iter().collect(),
                })
            })
            .collect::<Vec<_>>();

        tokio::task::spawn_local(async move {
            let _ = future::join_all(futs).await;
        });

        Ok(())
    }
}

impl Handler<GetConnections> for Vpn {
    type Result = <GetConnections as Message>::Result;

    fn handle(&mut self, _: GetConnections, _: &mut Self::Context) -> Self::Result {
        Ok(self
            .connections
            .values()
            .map(|c| ya_client_model::net::Connection {
                protocol: c.meta.protocol as u16,
                local_ip: c.local.addr.to_string(),
                local_port: c.local.port,
                remote_ip: c.meta.remote.addr.to_string(),
                remote_port: c.meta.remote.port,
            })
            .collect())
    }
}

impl Handler<Connect> for Vpn {
    type Result = ActorResponse<Self, UserConnection, Error>;

    fn handle(&mut self, msg: Connect, ctx: &mut Self::Context) -> Self::Result {
        let remote = match to_ip(&msg.address) {
            Ok(ip) => IpEndpoint::new(ip.into(), msg.port),
            Err(err) => return ActorResponse::reply(Err(err)),
        };

        log::info!("VPN {}: connecting to {:?}", self.vpn.id(), remote);

        let connect = match self.stack.connect(remote) {
            Ok(fut) => fut,
            Err(err) => return ActorResponse::reply(Err(Error::ConnectionError(err.to_string()))),
        };

        self.poll(ctx.address());

        let meta = connect.meta.clone();
        let fut = async move {
            match tokio::time::timeout(TCP_CONN_TIMEOUT, connect).await {
                Ok(Ok(h)) => Ok(h),
                Ok(Err(e)) => Err(Error::ConnectionError(e.to_string())),
                Err(_) => Err(Error::ConnectionTimeout),
            }
        }
        .into_actor(self)
        .map(move |result, this, ctx| {
            let id = this.vpn.id();
            match result {
                Ok(local) => {
                    log::info!("VPN {}: connected to {:?}", id, remote);

                    let (tx, rx) = mpsc::channel(1);
                    let vpn = ctx.address().recipient();
                    let conn = Connection::new(meta.clone(), local, tx);
                    this.connections.insert(meta.handle, conn);

                    Ok(UserConnection { vpn, rx, meta })
                }
                Err(e) => {
                    log::warn!("VPN {}: cannot connect to {:?}: {}", id, remote, e);
                    ctx.address().do_send(Disconnect::with(meta.handle, &e));
                    Err(e)
                }
            }
        });

        ActorResponse::r#async(fut)
    }
}

impl Handler<Disconnect> for Vpn {
    type Result = <Disconnect as Message>::Result;

    fn handle(&mut self, msg: Disconnect, _: &mut Self::Context) -> Self::Result {
        let mut conn = match self.connections.remove(&msg.handle) {
            Some(conn) => conn,
            None => return Err(Error::ConnectionError("no connection".into())),
        };

        log::info!(
            "Dropping connection to {:?}: {:?}",
            conn.meta.remote,
            msg.reason
        );

        conn.tx.close_channel();
        self.stack
            .disconnect(conn.meta.protocol, conn.meta.handle)?;
        Ok(())
    }
}

/// Handle egress packet from the user
impl Handler<Packet> for Vpn {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, pkt: Packet, ctx: &mut Self::Context) -> Self::Result {
        if !self.connections.contains_key(&pkt.meta.handle) {
            return ActorResponse::reply(Err(Error::ConnectionError("no connection".into())));
        }
        let addr = ctx.address();
        let fut = self
            .stack
            .send(pkt.data, pkt.meta, move || addr.do_send(DataSent {}))
            .map_err(|e| Error::Other(e.to_string()));
        ActorResponse::r#async(fut.into_actor(self))
    }
}

/// Handle ingress packet from the network
impl Handler<RpcEnvelope<VpnPacket>> for Vpn {
    type Result = <RpcEnvelope<VpnPacket> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<VpnPacket>, ctx: &mut Self::Context) -> Self::Result {
        self.stack.receive_phy(msg.into_inner().0);
        self.poll(ctx.address());
        Ok(())
    }
}

impl Handler<DataSent> for Vpn {
    type Result = <DataSent as Message>::Result;

    fn handle(&mut self, _: DataSent, ctx: &mut Self::Context) -> Self::Result {
        self.poll(ctx.address());
        Ok(())
    }
}

impl Handler<Shutdown> for Vpn {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}

#[derive(Clone)]
struct Connection {
    meta: ConnectionMeta,
    local: IpEndpoint,
    tx: mpsc::Sender<Vec<u8>>,
}

impl Connection {
    pub fn new(meta: ConnectionMeta, local: IpEndpoint, tx: mpsc::Sender<Vec<u8>>) -> Self {
        Self { meta, local, tx }
    }
}

trait SocketExt {
    fn is_open(&self) -> bool;

    fn can_recv(&self) -> bool;
    fn recv(&mut self) -> std::result::Result<Option<(IpEndpoint, Vec<u8>)>, smoltcp::Error>;

    fn can_send(&self) -> bool;
    fn send_capacity(&self) -> usize;
    fn send_queue(&self) -> usize;
}

impl<'a> SocketExt for Socket<'a> {
    fn is_open(&self) -> bool {
        match &self {
            Self::Tcp(s) => s.is_open(),
            Self::Udp(s) => s.is_open(),
            Self::Icmp(s) => s.is_open(),
            Self::Raw(_) => true,
        }
    }

    fn can_recv(&self) -> bool {
        match &self {
            Self::Tcp(s) => s.can_recv(),
            Self::Udp(s) => s.can_recv(),
            Self::Icmp(s) => s.can_recv(),
            Self::Raw(s) => s.can_recv(),
        }
    }

    fn recv(&mut self) -> std::result::Result<Option<(IpEndpoint, Vec<u8>)>, smoltcp::Error> {
        let result = match self {
            Self::Tcp(tcp) => tcp
                .recv(|bytes| (bytes.len(), bytes.to_vec()))
                .map(|vec| (tcp.remote_endpoint(), vec)),
            Self::Udp(udp) => udp
                .recv()
                .map(|(bytes, endpoint)| (endpoint, bytes.to_vec())),
            Self::Icmp(icmp) => icmp
                .recv()
                .map(|(bytes, address)| ((address, 0).into(), bytes.to_vec())),
            Self::Raw(raw) => raw
                .recv()
                .map(|bytes| (IpEndpoint::default(), bytes.to_vec())),
        };

        match result {
            Ok(tuple) => Ok(Some(tuple)),
            Err(smoltcp::Error::Exhausted) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn can_send(&self) -> bool {
        match &self {
            Self::Tcp(s) => s.can_send(),
            Self::Udp(s) => s.can_send(),
            Self::Icmp(s) => s.can_send(),
            Self::Raw(s) => s.can_send(),
        }
    }

    fn send_capacity(&self) -> usize {
        match &self {
            Self::Tcp(s) => s.send_capacity(),
            Self::Udp(s) => s.payload_send_capacity(),
            Self::Icmp(s) => s.payload_send_capacity(),
            Self::Raw(s) => s.payload_send_capacity(),
        }
    }

    fn send_queue(&self) -> usize {
        match &self {
            Self::Tcp(s) => s.send_queue(),
            _ => {
                if self.can_send() {
                    self.send_capacity() // mock value
                } else {
                    0
                }
            }
        }
    }
}

fn net_route(ip: IpAddr) -> Result<Route> {
    Ok(match ip {
        IpAddr::V4(a) => Route::new_ipv4_gateway(a.into()),
        IpAddr::V6(a) => Route::new_ipv6_gateway(a.into()),
    })
}

fn gsb_local_url(net_id: &str) -> String {
    format!("/public/vpn/{}", net_id)
}

fn gsb_remote_url(node_id: &str, net_id: &str) -> Endpoint {
    typed::service(format!("/udp/net/{}/vpn/{}", node_id, net_id))
}

trait ArbiterExt {
    fn spawn_ext<'a, F, T, E>(self, f: F) -> BoxFuture<'a, std::result::Result<T, E>>
    where
        F: Future<Output = std::result::Result<T, E>> + Send + 'static,
        T: Send + 'static,
        E: Send + From<Canceled> + 'static;
}

impl ArbiterExt for Arbiter {
    fn spawn_ext<'a, F, T, E>(self, f: F) -> BoxFuture<'a, std::result::Result<T, E>>
    where
        F: Future<Output = std::result::Result<T, E>> + Send + 'static,
        T: Send + 'static,
        E: Send + From<Canceled> + 'static,
    {
        let (tx, rx) = oneshot::channel();

        let tx_fut = async move {
            let _ = tx.send(f.await);
        };
        let rx_fut = rx.then(|r| async move {
            match r {
                Ok(r) => r,
                Err(e) => Err(e.into()),
            }
        });

        self.send(Box::pin(tx_fut));
        Box::pin(rx_fut)
    }
}

#[cfg(test)]
mod tests {
    use crate::network::VpnSupervisor;
    use ya_client_model::net::NewNetwork;
    use ya_core_model::NodeId;

    #[actix_rt::test]
    async fn create_remove_network() -> anyhow::Result<()> {
        let node_id = NodeId::default();

        let mut supervisor = VpnSupervisor::default();
        let network = supervisor
            .create_network(
                &node_id,
                NewNetwork {
                    ip: "10.0.0.0".to_string(),
                    mask: None,
                    gateway: None,
                },
            )
            .await?;

        supervisor.get_network(&node_id, &network.id)?;
        supervisor.remove_network(&node_id, &network.id)?;

        Ok(())
    }
}
