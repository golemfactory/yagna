use crate::interface::{add_iface_address, add_iface_route, default_iface, CaptureInterface};
use crate::message::*;
use crate::Result;
use actix::prelude::*;
use actix_web::error::Canceled;
use futures::channel::{mpsc, oneshot};
use futures::future;
use futures::future::BoxFuture;
use futures::{FutureExt, SinkExt, StreamExt};
use rand::distributions::{Distribution, Uniform};
use smoltcp::iface::Route;
use smoltcp::socket::{
    IcmpSocket, Socket, SocketHandle, SocketSet, TcpSocket, TcpSocketBuffer, UdpSocket,
};
use smoltcp::time::{Duration, Instant};
use smoltcp::wire::{IpAddress, IpCidr, IpEndpoint};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::TryFrom;
use std::net::IpAddr;
use std::ops::{DerefMut, RangeInclusive};
use std::str::FromStr;
use ya_core_model::activity::{VpnControl, VpnPacket};
use ya_core_model::NodeId;
use ya_service_bus::typed::{self, Endpoint};
use ya_service_bus::{actix_rpc, RpcEndpoint, RpcEnvelope};
use ya_utils_networking::vpn::common::{to_ip, to_net};
use ya_utils_networking::vpn::*;

// (protocol, local address, local port, remote address, remote port)
pub type SocketTuple = (Protocol, IpAddress, u16, IpAddress, u16);
const TCP_CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);

pub struct VpnSupervisor {
    networks: HashMap<String, Addr<Vpn>>,
    blueprints: HashMap<String, ya_client_model::net::Network>,
    ownership: HashMap<NodeId, BTreeSet<String>>,
    arbiter: Arbiter,
}

impl Default for VpnSupervisor {
    fn default() -> Self {
        Self {
            networks: Default::default(),
            blueprints: Default::default(),
            ownership: Default::default(),
            arbiter: Arbiter::new(),
        }
    }
}

impl VpnSupervisor {
    pub fn get_networks<'a>(&mut self, node_id: &NodeId) -> Vec<ya_client_model::net::Network> {
        self.ownership
            .get(node_id)
            .map(|ns| {
                ns.iter()
                    .filter_map(|id| self.blueprints.get(id.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_else(Vec::new)
    }

    pub fn get_network<'a>(
        &mut self,
        node_id: &NodeId,
        network_id: &str,
    ) -> Result<ya_client_model::net::Network> {
        self.verify_owner_id(node_id, network_id)?;
        self.blueprints
            .get(network_id)
            .cloned()
            .ok_or_else(|| Error::NetNotFound(network_id.to_owned()))
    }

    pub async fn create_network(
        &mut self,
        node_id: &NodeId,
        network: ya_client_model::net::Network,
    ) -> Result<()> {
        if self.networks.contains_key(&network.id) {
            return Err(Error::NetIdTaken(network.id));
        }

        let def = network.clone();
        let net = to_net(&network.ip, network.mask.as_ref())?;
        let net_ip = IpCidr::new(net.addr().into(), net.prefix_len());
        let net_gw = match network.gateway.map(|g| IpAddr::from_str(&g)).transpose()? {
            Some(gw) => gw,
            None => net
                .hosts()
                .next()
                .ok_or_else(|| Error::NetCidr(net.addr(), net.prefix_len()))?,
        };

        let route = net_route(net_gw)?;
        let net = Network::new(&network.id, net);
        let mut stack = default_iface();
        add_iface_route(&mut stack, net_ip, route);

        let vpn = self
            .arbiter
            .clone()
            .spawn_ext(async move { Ok::<_, Error>(Vpn::new(stack, net).start()) })
            .await?;

        self.networks.insert(network.id.clone(), vpn);
        self.blueprints.insert(network.id.clone(), def);
        self.ownership
            .entry(node_id.clone())
            .or_insert_with(Default::default)
            .insert(network.id);

        Ok(())
    }

    pub fn remove_network<'a>(
        &mut self,
        node_id: &NodeId,
        network_id: &str,
    ) -> Result<BoxFuture<'a, Result<()>>> {
        self.verify_owner_id(node_id, network_id)?;
        self.networks.remove(network_id);
        self.blueprints.remove(network_id);
        self.forward(network_id, Shutdown {})
    }

    pub fn get_addresses<'a>(
        &self,
        node_id: &NodeId,
        network_id: &str,
    ) -> Result<BoxFuture<'a, Result<Vec<ya_client_model::net::Address>>>> {
        self.verify_owner_id(node_id, network_id)?;
        self.forward(network_id, GetAddresses {})
    }

    pub fn add_address<'a>(
        &self,
        node_id: &NodeId,
        network_id: &str,
        address: String,
    ) -> Result<BoxFuture<'a, Result<()>>> {
        self.verify_owner_id(node_id, network_id)?;
        self.forward(network_id, AddAddress { address })
    }

    pub fn get_nodes<'a>(
        &self,
        node_id: &NodeId,
        network_id: &str,
    ) -> Result<BoxFuture<'a, Result<Vec<ya_client_model::net::Node>>>> {
        self.verify_owner_id(node_id, network_id)?;
        self.forward(network_id, GetNodes {})
    }

    pub fn add_node<'a>(
        &self,
        node_id: &NodeId,
        network_id: &str,
        id: String,
        address: String,
    ) -> Result<BoxFuture<'a, Result<()>>> {
        self.verify_owner_id(node_id, network_id)?;
        self.forward(network_id, AddNode { id, address })
    }

    pub fn remove_node<'a>(
        &self,
        node_id: &NodeId,
        network_id: &str,
        id: String,
    ) -> Result<BoxFuture<'a, Result<()>>> {
        self.verify_owner_id(node_id, network_id)?;
        self.forward(network_id, RemoveNode { id })
    }

    pub fn get_connections<'a>(
        &self,
        node_id: &NodeId,
        network_id: &str,
    ) -> Result<BoxFuture<'a, Result<Vec<ya_client_model::net::Connection>>>> {
        self.verify_owner_id(node_id, network_id)?;
        self.forward(network_id, GetConnections {})
    }

    pub fn connect_tcp<'a>(
        &self,
        node_id: &NodeId,
        network_id: &str,
        ip: &str,
        port: u16,
    ) -> Result<BoxFuture<'a, Result<(mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>)>>> {
        self.verify_owner_id(node_id, network_id)?;
        let vpn = self.network_actor(network_id)?;
        let network_id = network_id.to_string();

        let (ws_tx, ws_rx) = mpsc::channel(1);
        let connect = ConnectTcp {
            receiver: ws_rx,
            address: ip.to_string(),
            port,
        };

        Ok(self.arbiter.clone().spawn_ext(async move {
            let vpn_rx = vpn
                .send(connect)
                .await
                .map_err(|_| Error::NetNotFound(network_id))??;
            Ok((ws_tx, vpn_rx))
        }))
    }

    fn network_actor(&self, network_id: &str) -> Result<Addr<Vpn>> {
        self.networks
            .get(network_id)
            .cloned()
            .ok_or_else(|| Error::NetNotFound(network_id.to_owned()))
    }

    fn forward<'a, M, T>(
        &self,
        network_id: &str,
        msg: M,
    ) -> Result<BoxFuture<'a, <M as Message>::Result>>
    where
        Vpn: Handler<M>,
        M: Message<Result = std::result::Result<T, Error>> + Send + 'static,
        <M as Message>::Result: Send + 'static,
        T: Send + 'static,
    {
        let arbiter = self.arbiter.clone();
        let vpn = self.network_actor(network_id)?;

        let network_id = network_id.to_string();
        let fut = arbiter.spawn_ext(async move {
            match vpn.send(msg).await {
                Ok(r) => r,
                Err(_) => Err(Error::NetNotFound(network_id)),
            }
        });
        Ok(Box::pin(fut))
    }

    fn verify_owner_id(&self, node_id: &NodeId, network_id: &str) -> Result<()> {
        self.ownership
            .get(node_id)
            .map(|s| s.contains(network_id))
            .ok_or_else(|| Error::NetNotFound(network_id.to_string()))?
            .then(|| ())
            .ok_or_else(|| Error::Forbidden)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum VpnState {
    Idle,
    Started,
    Stopping,
    Stopped,
}

pub struct Vpn {
    vpn: Network<Endpoint>,
    stack: CaptureInterface<'static>,
    sockets: SocketSet<'static>,
    ports: Ports,
    connections: HashMap<SocketTuple, Connection>,
    pending: HashMap<SocketTuple, PendingConnection>,
    state: VpnState,
}

impl Vpn {
    pub fn new(stack: CaptureInterface<'static>, vpn: Network<Endpoint>) -> Self {
        Self {
            vpn,
            stack,
            sockets: SocketSet::new(vec![]),
            ports: Default::default(),
            connections: Default::default(),
            pending: Default::default(),
            state: VpnState::Idle,
        }
    }

    fn process(
        &mut self,
        to_receive: Option<Vec<u8>>,
        mut to_send: Option<Packet>,
        addr: Addr<Self>,
    ) {
        if let Some(frame) = to_receive {
            self.stack.device_mut().phy_tx(frame);
        }

        loop {
            if let Err(err) = self.stack.poll(&mut self.sockets, Instant::now()) {
                log::warn!("VPN {}: socket poll error: {}", self.vpn.id(), err);
            }

            let processed_ingress = self.process_ingress(addr.clone());
            let processed_egress = self.process_egress(&mut to_send);

            if !processed_ingress && !processed_egress {
                break;
            }
        }
    }

    fn process_ingress(&mut self, actor: Addr<Self>) -> bool {
        let mut processed = false;

        let sockets = &mut self.sockets;
        let connections = &self.connections;

        for mut socket_ref in sockets.iter_mut() {
            let socket: &mut Socket = socket_ref.deref_mut();

            if !socket.is_open() {
                actor.do_send(Disconnect {
                    handle: socket.handle(),
                    reason: DisconnectReason::SocketClosed,
                });
                continue;
            }

            if socket.can_send() {
                if let Some(tuple) = socket.tuple() {
                    if let Some(tx) = self
                        .pending
                        .remove(&tuple)
                        .map(|mut p| p.ready_tx.take())
                        .flatten()
                    {
                        let _ = tx.send(Ok(()));
                    }
                }
            }

            while socket.can_recv() {
                let (addr, port, data) = match socket.recv() {
                    Ok(Some(t)) => t,
                    Ok(None) => break,
                    Err(e) => {
                        log::error!("VPN {}: packet error: {}", self.vpn.id(), e);
                        continue;
                    }
                };

                processed = true;

                let conn = socket.tuple().map(|t| connections.get(&t)).flatten();
                let mut user_tx = match conn {
                    Some(conn) => conn.user_tx.clone(),
                    None => {
                        log::warn!("VPN {}: no connection to {}:{}", self.vpn.id(), addr, port);
                        continue;
                    }
                };

                let addr_ = actor.clone();
                let handle_ = socket.handle();
                tokio::task::spawn_local(async move {
                    if let Err(_) = user_tx.send(data).await {
                        let _ = addr_
                            .send(Disconnect {
                                handle: handle_,
                                reason: DisconnectReason::SinkClosed,
                            })
                            .await;
                    }
                });
            }
        }

        processed
    }

    fn process_egress(&mut self, to_send: &mut Option<Packet>) -> bool {
        let mut processed = false;
        let vpn_id = self.vpn.id().clone();

        if let Some(packet) = to_send.take() {
            let (ip, port) = (packet.socket_tuple.3, packet.socket_tuple.4);
            if self.send_packet(packet) {
                processed = true;
            } else {
                log::warn!(
                    "VPN {}: unable to send packet to {}:{}: no connection",
                    vpn_id,
                    ip,
                    port,
                );
            }
        }

        let device = self.stack.device_mut();
        while let Some(data) = device.next_phy_rx() {
            log::trace!("Processing egress phy packet");
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
                    log::debug!("Egress IP packet to {:?}", packet.dst_address());
                    self.vpn.endpoint(packet.dst_address())
                }
                EtherFrame::Arp(_) => {
                    let packet = ArpPacket::packet(frame.payload());
                    log::debug!("Egress ARP packet to {:?}", packet.get_field(ArpField::TPA));
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

            let vpn_id_ = vpn_id.clone();
            tokio::task::spawn_local(async move {
                if let Err(err) = endpoint.send(VpnPacket(frame.into())).await {
                    log::warn!(
                        "VPN {}: error sending packet to endpoint '{}': {}",
                        vpn_id_,
                        endpoint.addr(),
                        err
                    );
                }
            });
        }

        processed
    }

    fn send_packet(&mut self, packet: Packet) -> bool {
        let mut processed = false;

        let handle = match self.connections.get(&packet.socket_tuple) {
            Some(conn) => &conn.handle,
            None => return false,
        };
        let (proto, ip, port) = (
            packet.socket_tuple.0,
            packet.socket_tuple.3,
            packet.socket_tuple.4,
        );

        log::warn!("Send packet to {:?}", packet.socket_tuple);

        let result = match proto {
            Protocol::Tcp => self
                .sockets
                .get::<TcpSocket>(*handle)
                .send_slice(&packet.data),
            Protocol::Udp => {
                let endpoint = IpEndpoint::new(ip, port);
                self.sockets
                    .get::<UdpSocket>(*handle)
                    .send_slice(&packet.data, endpoint)
                    .map(|_| packet.data.len())
            }
            Protocol::Icmp => self
                .sockets
                .get::<IcmpSocket>(*handle)
                .send_slice(&packet.data, ip)
                .map(|_| packet.data.len()),
            _ => {
                self.log_send_err(ip, port, format!("protocol not supported: {:?}", proto));
                return false;
            }
        };

        match result {
            Ok(size) => {
                processed = true;
                if size < packet.data.len() {
                    self.log_send_err(ip, port, "no space in buffer");
                }
            }
            Err(smoltcp::Error::Exhausted) => (),
            Err(err) => {
                processed = true;
                self.log_send_err(ip, port, err);
            }
        }

        processed
    }

    fn socket_tuple(
        &mut self,
        protocol: Protocol,
        remote_ip: &str,
        remote_port: Option<u16>,
    ) -> Result<SocketTuple> {
        let local_ip: IpAddress = self.vpn.address()?.into();
        let local_port = self.ports.next(protocol)?;
        let remote_ip: IpAddress = to_ip(remote_ip)?.into();
        let remote_port = remote_port.unwrap_or(0);
        Ok((protocol, local_ip, local_port, remote_ip, remote_port))
    }

    fn log_send_err<E: ToString>(&self, ip: IpAddress, port: u16, msg: E) {
        log::warn!(
            "VPN {}: unable to send packet to {}:{}: {}",
            self.vpn.id(),
            ip,
            port,
            msg.to_string()
        );
    }
}

impl Actor for Vpn {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let vpn_id = gsb_url(self.vpn.id());
        actix_rpc::bind::<VpnPacket>(&vpn_id, ctx.address().recipient());

        self.state = VpnState::Started;
        log::info!("VPN {} started", self.vpn.id());
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        if self.state != VpnState::Stopping {
            return Running::Continue;
        }

        let id = self.vpn.id().clone();
        let vpn_id = gsb_url(&id);

        async move {
            log::debug!("Stopping VPN {}", id);
            let _ = typed::unbind(&vpn_id).await;
        }
        .into_actor(self)
        .wait(ctx);

        Running::Stop
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        self.state = VpnState::Stopped;
        log::info!("VPN {} stopped", self.vpn.id());
    }
}

impl Handler<GetAddresses> for Vpn {
    type Result = <GetAddresses as Message>::Result;

    fn handle(&mut self, _: GetAddresses, _: &mut Self::Context) -> Self::Result {
        Ok(self
            .stack
            .ip_addrs()
            .iter()
            .map(|ip| ya_client_model::net::Address { ip: ip.to_string() })
            .collect())
    }
}

impl Handler<AddAddress> for Vpn {
    type Result = <AddAddress as Message>::Result;

    fn handle(&mut self, msg: AddAddress, _: &mut Self::Context) -> Self::Result {
        let ip_addr: IpAddr = msg.address.parse()?;
        let ip_address = IpAddress::from(ip_addr);

        let network = self.vpn.as_ref();
        if !network.contains(&ip_addr) {
            return Err(Error::NetAddrMismatch(ip_addr));
        }

        let cidr = IpCidr::new(ip_address, network.prefix_len());
        if !cidr.address().is_unicast() && !cidr.address().is_unspecified() {
            return Err(Error::IpAddrNotAllowed(msg.address.parse()?));
        }

        add_iface_address(&mut self.stack, cidr);
        self.vpn.add_address(&msg.address)
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
        self.vpn.add_node(ip, &msg.id, gsb_remote_url)?;

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
            .keys()
            .map(|(p, la, lp, ra, rp)| ya_client_model::net::Connection {
                protocol: *p as u16,
                local_ip: la.to_string(),
                local_port: *lp,
                remote_ip: ra.to_string(),
                remote_port: *rp,
            })
            .collect())
    }
}

impl Handler<ConnectTcp> for Vpn {
    type Result = ActorResponse<Self, mpsc::Receiver<Vec<u8>>, Error>;

    fn handle(&mut self, msg: ConnectTcp, ctx: &mut Self::Context) -> Self::Result {
        let timeout = TCP_CONNECTION_TIMEOUT;
        let protocol = Protocol::Tcp;
        let tuple = match self.socket_tuple(protocol, &msg.address, Some(msg.port)) {
            Ok(t) => t,
            Err(e) => return ActorResponse::reply(Err(e)),
        };

        let tcp_socket = {
            let tcp_rx = TcpSocketBuffer::new(vec![0; MAX_FRAME_SIZE * 4]);
            let tcp_tx = TcpSocketBuffer::new(vec![0; MAX_FRAME_SIZE * 4]);
            let mut socket = TcpSocket::new(tcp_rx, tcp_tx);
            socket.set_keep_alive(Some(Duration::from_secs(60)));
            socket
        };
        let handle = self.sockets.add(tcp_socket);

        if let Err(e) = {
            let mut socket = self.sockets.get::<TcpSocket>(handle);
            socket.connect((tuple.3, tuple.4), (tuple.1, tuple.2))
        } {
            self.sockets.remove(handle);
            self.ports.free(tuple.0, tuple.2);

            let result = Err(Error::Other(e.to_string()));
            return ActorResponse::reply(result);
        }

        let (tx, rx) = mpsc::channel(8);
        let (ready_tx, ready_rx) = oneshot::channel();

        self.pending.insert(tuple, PendingConnection::new(ready_tx));
        self.process(None, None, ctx.address());

        log::debug!("VPN {}: connecting to {:?}", self.vpn.id(), tuple);

        let connect = async move {
            match tokio::time::timeout(timeout.into(), ready_rx).await {
                Ok(Ok(_)) => Ok(rx),
                Ok(Err(e)) => Err(Error::ConnectionError(e.to_string())),
                Err(_elapsed) => Err(Error::ConnectionTimeout),
            }
        }
        .into_actor(self)
        .map(move |result, this, ctx| {
            this.pending.remove(&tuple);

            match &result {
                Ok(_) => {
                    log::debug!("VPN {}: connected to {:?}", this.vpn.id(), tuple);
                    this.connections.insert(tuple, Connection::new(handle, tx));
                    ctx.add_stream(StreamExt::map(msg.receiver, move |data| Packet {
                        socket_tuple: tuple,
                        data,
                    }));
                }
                Err(e) => {
                    log::debug!(
                        "VPN {}: connection to {:?} failed: {}",
                        this.vpn.id(),
                        tuple,
                        e
                    );
                    ctx.address().do_send(Disconnect {
                        handle,
                        reason: match e {
                            Error::ConnectionTimeout => DisconnectReason::ConnectionTimeout,
                            _ => DisconnectReason::ConnectionFailed,
                        },
                    });
                }
            }
            result
        });
        ActorResponse::r#async(connect)
    }
}

impl Handler<Disconnect> for Vpn {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: Disconnect, _: &mut Self::Context) -> Self::Result {
        let fut = self
            .sockets
            .remove(msg.handle)
            .tuple()
            .map(|t| {
                log::debug!("Dropping connection to {:?}: {:?}", t, msg.reason);

                self.ports.free(t.0, t.2);
                self.connections.remove(&t);

                if let Some(mut pending) = self.pending.remove(&t) {
                    if let Some(tx) = pending.ready_tx.take() {
                        return async move {
                            let err = Error::ConnectionError(format!("{:?}", msg.reason));
                            let _ = tx.send(Err(err));
                            Ok(())
                        }
                        .boxed_local();
                    }
                }

                future::ok(()).boxed_local()
            })
            .unwrap_or_else(|| future::ok(()).boxed_local())
            .into_actor(self);

        ActorResponse::r#async(fut)
    }
}

/// Handle egress packet from the user
impl StreamHandler<Packet> for Vpn {
    fn handle(&mut self, pkt: Packet, ctx: &mut Self::Context) {
        self.process(None, Some(pkt), ctx.address());
    }
}

/// Handle ingress packet from the network
impl Handler<RpcEnvelope<VpnPacket>> for Vpn {
    type Result = <RpcEnvelope<VpnPacket> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<VpnPacket>, ctx: &mut Self::Context) -> Self::Result {
        let data = msg.into_inner().0;
        self.process(Some(data), None, ctx.address());
        Ok(())
    }
}

impl Handler<Shutdown> for Vpn {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        self.state = VpnState::Stopping;
        ctx.stop();
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Ports {
    taken: BTreeMap<Protocol, BTreeSet<u16>>,
}

impl Ports {
    const RANGE: RangeInclusive<u16> = 1000..=65535;

    pub fn next(&mut self, proto: Protocol) -> Result<u16> {
        let mut rng = rand::thread_rng();
        let mut port = Uniform::from(Self::RANGE).sample(&mut rng);
        let taken = self.taken.entry(proto).or_insert_with(Default::default);

        let range_start = *Self::RANGE.start();
        let mut num = Self::RANGE.len() as i32;

        while num > 0 {
            if !taken.contains(&port) {
                taken.insert(port);
                return Ok(port);
            }
            port = range_start.max(port.overflowing_add(1).0);
            num -= 1;
        }

        Err(Error::Other("No free ports available".into()))
    }

    #[allow(unused)]
    pub fn reserve(&mut self, proto: Protocol, port: u16) -> Result<()> {
        let taken = self.taken.entry(proto).or_insert_with(Default::default);
        if taken.contains(&port) {
            return Err(Error::Other(format!("Port {} is not available", port)));
        }
        taken.insert(port);
        Ok(())
    }

    pub fn free(&mut self, proto: Protocol, port: u16) {
        self.taken
            .entry(proto)
            .or_insert_with(Default::default)
            .remove(&port);
    }
}

struct PendingConnection {
    pub ready_tx: Option<oneshot::Sender<Result<()>>>,
}

impl PendingConnection {
    pub fn new(ready_tx: oneshot::Sender<Result<()>>) -> Self {
        Self {
            ready_tx: Some(ready_tx),
        }
    }
}

struct Connection {
    pub handle: SocketHandle,
    pub user_tx: mpsc::Sender<Vec<u8>>,
}

impl Connection {
    pub fn new(handle: SocketHandle, user_tx: mpsc::Sender<Vec<u8>>) -> Self {
        Self { handle, user_tx }
    }
}

struct Packet {
    pub socket_tuple: SocketTuple,
    pub data: Vec<u8>,
}

trait SocketExt {
    fn tuple(&self) -> Option<SocketTuple>;
    fn is_open(&self) -> bool;

    fn can_recv(&self) -> bool;
    fn can_send(&self) -> bool;

    fn recv(&mut self) -> std::result::Result<Option<(IpAddress, u16, Vec<u8>)>, smoltcp::Error>;
}

impl<'a> SocketExt for Socket<'a> {
    fn tuple(&self) -> Option<SocketTuple> {
        match &self {
            Self::Tcp(s) => Some(tcp_socket_tuple(s)),
            Self::Udp(s) => Some(udp_socket_tuple(s)),
            Self::Icmp(s) => Some(icmp_socket_tuple(s)),
            Self::Raw(_) => None,
        }
    }

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

    fn can_send(&self) -> bool {
        match &self {
            Self::Tcp(s) => s.can_send(),
            Self::Udp(s) => s.can_send(),
            Self::Icmp(s) => s.can_send(),
            Self::Raw(s) => s.can_send(),
        }
    }

    fn recv(&mut self) -> std::result::Result<Option<(IpAddress, u16, Vec<u8>)>, smoltcp::Error> {
        let result = match self {
            Self::Tcp(s) => s.recv(|bytes| (bytes.len(), bytes.to_vec())).map(|data| {
                let endpoint = s.remote_endpoint();
                (endpoint.addr, endpoint.port, data)
            }),
            Self::Udp(s) => s
                .recv()
                .map(|(bytes, endpoint)| (endpoint.addr, endpoint.port, bytes.to_vec())),
            Self::Icmp(s) => s
                .recv()
                .map(|(bytes, address)| (address, 0, bytes.to_vec())),
            Self::Raw(s) => s
                .recv()
                .map(|bytes| (IpAddress::Unspecified, 0, bytes.to_vec())),
        };
        match result {
            Ok(tuple) => Ok(Some(tuple)),
            Err(smoltcp::Error::Exhausted) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

fn tcp_socket_tuple(socket: &TcpSocket) -> SocketTuple {
    let local = socket.local_endpoint();
    let remote = socket.remote_endpoint();
    (
        Protocol::Tcp,
        local.addr,
        local.port,
        remote.addr,
        remote.port,
    )
}

fn udp_socket_tuple(socket: &UdpSocket) -> SocketTuple {
    let local = socket.endpoint();
    (
        Protocol::Udp,
        local.addr,
        local.port,
        IpAddress::Unspecified,
        0,
    )
}

fn icmp_socket_tuple(_: &IcmpSocket) -> SocketTuple {
    (
        Protocol::Icmp,
        IpAddress::Unspecified,
        0,
        IpAddress::Unspecified,
        0,
    )
}

fn net_route(ip: IpAddr) -> Result<Route> {
    Ok(match ip {
        IpAddr::V4(a) => Route::new_ipv4_gateway(a.into()),
        IpAddr::V6(a) => Route::new_ipv6_gateway(a.into()),
    })
}

fn gsb_url(net_id: &str) -> String {
    format!("/public/vpn/{}", net_id)
}

fn gsb_remote_url(node_id: &str, net_id: &str) -> Endpoint {
    typed::service(format!("/net/{}/vpn/{}", node_id, net_id))
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
