use crate::interface::{add_iface_address, add_iface_route, default_iface, CaptureInterface};
use crate::message::*;
use crate::Result;
use actix::prelude::*;
use futures::channel::{mpsc, oneshot};
use futures::future::{self, LocalBoxFuture};
use futures::{FutureExt, SinkExt, StreamExt};
use ipnet::IpNet;
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
use ya_core_model::activity::{VpnControl, VpnPacket};
use ya_service_bus::typed::{self, Endpoint};
use ya_service_bus::{actix_rpc, RpcEndpoint, RpcEnvelope};
use ya_utils_networking::vpn::common::{to_ip, to_net};
use ya_utils_networking::vpn::{
    ArpField, ArpPacket, Error, EtherFrame, IpPacket, Network, PeekPacket, Protocol, MAX_FRAME_SIZE,
};

// (protocol, local address, local port, remote address, remote port)
pub type SocketTuple = (Protocol, IpAddress, u16, IpAddress, u16);
const TCP_CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct VpnSupervisor {
    networks: HashMap<String, Addr<Vpn>>,
}

impl Actor for VpnSupervisor {
    type Context = Context<Self>;

    fn started(&mut self, _: &mut Self::Context) {
        log::info!("VPN supervisor started");
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        let networks = std::mem::replace(&mut self.networks, Default::default());
        let futures = networks.into_iter().map(|(_, a)| a.send(Shutdown {}));

        future::join_all(futures)
            .then(|_| future::ready(()))
            .into_actor(self)
            .wait(ctx);

        Running::Stop
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::info!("VPN supervisor stopped");
    }
}

impl Supervised for VpnSupervisor {
    fn restarting(&mut self, _ctx: &mut Self::Context) {
        log::info!("VPN supervisor is restarting");
    }
}

impl SystemService for VpnSupervisor {}

impl Handler<VpnCreateNetwork> for VpnSupervisor {
    type Result = <VpnCreateNetwork as Message>::Result;

    fn handle(&mut self, msg: VpnCreateNetwork, _: &mut Self::Context) -> Self::Result {
        if self.networks.contains_key(&msg.network.id) {
            return Err(Error::NetIdTaken(msg.network.id));
        }
        let net = to_net(
            &msg.network.address,
            &msg.network.mask.unwrap_or_else(|| "255.255.255.0".into()),
        )?;

        let net_ip = IpCidr::new(net.addr().into(), net.prefix_len());
        let node_ip = node_addr(&msg.requestor_address, &net)?;
        let route = net_route(&net)?;

        let mut stack = default_iface();
        add_iface_address(&mut stack, node_ip);
        add_iface_route(&mut stack, net_ip, route);

        let net = Network::new(&msg.network.id, net);
        let vpn = Vpn::new(stack, net).start();

        vpn.do_send(VpnAddAddress {
            net_id: msg.network.id.clone(),
            address: node_ip.address().to_string(),
        });

        self.networks.insert(msg.network.id, vpn);
        Ok(())
    }
}

impl Handler<VpnGetNetwork<Vpn>> for VpnSupervisor {
    type Result = <VpnGetNetwork<Vpn> as Message>::Result;

    fn handle(&mut self, msg: VpnGetNetwork<Vpn>, _: &mut Self::Context) -> Self::Result {
        match self.networks.get(&msg.net_id) {
            Some(addr) => Ok(addr.clone()),
            None => Err(Error::NetNotFound(msg.net_id)),
        }
    }
}

impl Handler<VpnRemoveNetwork> for VpnSupervisor {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: VpnRemoveNetwork, _: &mut Self::Context) -> Self::Result {
        match self.forward(msg.net_id, Shutdown {}, "shutting down") {
            Ok(fut) => ActorResponse::r#async(fut.into_actor(self)),
            Err(err) => ActorResponse::reply(Err(err)),
        }
    }
}

impl Handler<VpnAddAddress> for VpnSupervisor {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: VpnAddAddress, _: &mut Self::Context) -> Self::Result {
        match self.forward(msg.net_id.clone(), msg, "adding address") {
            Ok(fut) => ActorResponse::r#async(fut.into_actor(self)),
            Err(err) => ActorResponse::reply(Err(err)),
        }
    }
}

impl Handler<VpnAddNode> for VpnSupervisor {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: VpnAddNode, _: &mut Self::Context) -> Self::Result {
        match self.forward(msg.net_id.clone(), msg, "adding node") {
            Ok(fut) => ActorResponse::r#async(fut.into_actor(self)),
            Err(err) => ActorResponse::reply(Err(err)),
        }
    }
}

impl Handler<VpnRemoveNode> for VpnSupervisor {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: VpnRemoveNode, _: &mut Self::Context) -> Self::Result {
        match self.forward(msg.net_id.clone(), msg, "removing node") {
            Ok(fut) => ActorResponse::r#async(fut.into_actor(self)),
            Err(err) => ActorResponse::reply(Err(err)),
        }
    }
}

impl VpnSupervisor {
    fn forward<T>(
        &self,
        net_id: String,
        msg: T,
        action: &str,
    ) -> Result<LocalBoxFuture<'static, Result<()>>>
    where
        T: Message + Send + 'static,
        <T as Message>::Result: Send,
        Vpn: Handler<T>,
    {
        let addr = self
            .networks
            .get(&net_id)
            .cloned()
            .ok_or_else(|| Error::NetNotFound(net_id))?;

        let action = action.to_string();
        let fut = async move {
            if let Err(e) = addr.send(msg).await {
                log::warn!("Error: {} failed: {}", action, e);
            }
            Ok(())
        }
        .boxed_local();

        Ok(fut)
    }
}

pub struct Vpn {
    vpn: Network<Endpoint>,
    stack: CaptureInterface<'static>,
    sockets: SocketSet<'static>,
    ports: Ports,
    connections: HashMap<SocketTuple, Connection>,
    pending: HashMap<SocketTuple, PendingConnection>,
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
            log::info!("Processing egress phy packet");
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
                    log::info!("Processing egress IP packet to {:?}", packet.dst_address());
                    self.vpn.endpoint(packet.dst_address())
                }
                EtherFrame::Arp(_) => {
                    let packet = ArpPacket::packet(frame.payload());
                    log::info!(
                        "Processing egress ARP packet to {:?}",
                        packet.get_field(ArpField::TPA)
                    );
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
                    log::info!("No endpoint for egress packet");
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
                } else {
                    log::info!("Packet sent");
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
        log::info!("VPN {} started", self.vpn.id());
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
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
        log::info!("VPN {} stopped", self.vpn.id());
    }
}

impl Handler<VpnAddAddress> for Vpn {
    type Result = <VpnAddAddress as Message>::Result;

    fn handle(&mut self, msg: VpnAddAddress, _: &mut Self::Context) -> Self::Result {
        if &msg.net_id != self.vpn.id() {
            return Err(Error::Other("Invalid network ID".to_string()));
        }
        self.vpn.add_address(&msg.address)
    }
}

impl Handler<VpnAddNode> for Vpn {
    type Result = <VpnAddNode as Message>::Result;

    fn handle(&mut self, msg: VpnAddNode, _: &mut Self::Context) -> Self::Result {
        if &msg.net_id != self.vpn.id() {
            return Err(Error::Other("Invalid network ID".to_string()));
        }

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

impl Handler<VpnRemoveNode> for Vpn {
    type Result = <VpnRemoveNode as Message>::Result;

    fn handle(&mut self, msg: VpnRemoveNode, _: &mut Self::Context) -> Self::Result {
        if &msg.net_id != self.vpn.id() {
            return Err(Error::Other("Invalid network ID".to_string()));
        }

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
                        reason: DisconnectReason::ConnectionFailed,
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

fn node_addr(ip: &str, ip_net: &IpNet) -> Result<IpCidr> {
    let ip = to_ip(ip.as_ref())?;
    if !ip_net.contains(&ip) {
        return Err(Error::NetAddrMismatch(ip));
    }

    let cidr = IpCidr::new(ip.clone().into(), ip_net.prefix_len());
    if !cidr.address().is_unicast() && !cidr.address().is_unspecified() {
        return Err(Error::IpAddrNotAllowed(ip));
    }

    Ok(cidr)
}

fn net_route(ip_net: &IpNet) -> Result<Route> {
    let ip = ip_net
        .hosts()
        .next()
        .ok_or_else(|| Error::NetCidr(ip_net.addr(), ip_net.prefix_len()))?;
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
