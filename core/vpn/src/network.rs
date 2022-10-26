use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;
use std::rc::Rc;
use std::str::FromStr;

use actix::prelude::*;
use futures::channel::oneshot::Canceled;
use futures::channel::{mpsc, oneshot};
use futures::{future, future::BoxFuture, Future, FutureExt, SinkExt, StreamExt, TryFutureExt};
use smoltcp::iface::Route;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint};
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

use ya_utils_networking::vpn::socket::TCP_CONN_TIMEOUT;
use ya_utils_networking::vpn::stack::interface::{add_iface_address, add_iface_route, tap_iface};

use crate::message::*;
use crate::Result;

use ya_core_model::activity::{VpnControl, VpnPacket};
use ya_core_model::NodeId;
use ya_service_bus::typed::{self, Endpoint};
use ya_service_bus::{actix_rpc, RpcEndpoint, RpcEnvelope, RpcRawCall};
use ya_utils_networking::vpn::common::{to_ip, to_net};
use ya_utils_networking::vpn::stack::{
    self as net, EgressReceiver, IngressEvent, IngressReceiver, StackConfig,
};
use ya_utils_networking::vpn::*;

const DEFAULT_MAX_PACKET_SIZE: usize = 65536;

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
            .ok_or(Error::NetNotFound)
    }

    pub async fn create_network(
        &mut self,
        node_id: NodeId,
        network: ya_client_model::net::NewNetwork,
    ) -> Result<ya_client_model::net::Network> {
        let net = to_net(&network.ip, network.mask.as_ref())?;
        let node_ip = IpCidr::new(
            net.hosts()
                .next()
                .ok_or_else(|| Error::Other("No IP address found".into()))?
                .into(),
            net.prefix_len(),
        );

        let net_id = Uuid::new_v4().to_simple().to_string();
        let net_ip = IpCidr::new(net.addr().into(), net.prefix_len());
        let net_gw = match network
            .gateway
            .as_ref()
            .map(|g| IpAddr::from_str(g))
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
            .spawn_ext(async move {
                let vpn = Vpn::new(
                    node_id,
                    vpn_net,
                    create_stack_network(node_ip, net_ip, net_gw)?,
                );
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
            .entry(node_id)
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
        let vpn = self.networks.remove(network_id).ok_or(Error::NetNotFound)?;
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
            .ok_or(Error::NetNotFound)
    }

    fn owner(&self, node_id: &NodeId, network_id: &str) -> Result<()> {
        self.ownership
            .get(node_id)
            .map(|s| s.contains(network_id))
            .ok_or(Error::NetNotFound)?
            .then_some(())
            .ok_or(Error::Forbidden)
    }
}

pub struct Vpn {
    node_id: String,
    vpn: Network<network::DuoEndpoint<Endpoint>>,
    stack_network: net::Network,
    connections: HashMap<SocketDesc, InternalConnection>,
}

impl Vpn {
    pub fn new(
        node_id: NodeId,
        vpn: Network<network::DuoEndpoint<Endpoint>>,
        stack_network: net::Network,
    ) -> Self {
        Self {
            node_id: node_id.to_string(),
            vpn,
            stack_network,
            connections: Default::default(),
        }
    }
}

impl Actor for Vpn {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let id = self.vpn.id();
        let vpn_url = gsb_local_url(id);
        let addr = ctx.address();
        self.stack_network.spawn_local();

        actix_rpc::bind(&vpn_url, addr.clone().recipient());
        actix_rpc::bind_raw(&format!("{vpn_url}/raw"), addr.clone().recipient());

        let ingress_rx = self
            .stack_network
            .ingress_receiver()
            .expect("Ingress receiver already taken");

        let egress_rx = self
            .stack_network
            .egress_receiver()
            .expect("Egress receiver already taken");

        vpn_ingress_handler(ingress_rx, addr.clone(), id.clone())
            .into_actor(self)
            .spawn(ctx);

        vpn_egress_handler(egress_rx, addr, id.clone())
            .into_actor(self)
            .spawn(ctx);

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
            let _ = typed::unbind(&format!("{vpn_url}/raw")).await;
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
            .stack_network
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

        self.stack_network.stack.add_address(cidr);

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
            .flat_map(|(id, ips)| {
                ips.iter()
                    .map(|ip| ya_client_model::net::Node {
                        id: id.clone(),
                        ip: ip.to_string(),
                    })
                    .collect::<Vec<_>>()
            })
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
                e.tcp.send(VpnControl::AddNodes {
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
                e.tcp.send(VpnControl::RemoveNodes {
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

impl Handler<Connect> for Vpn {
    type Result = ActorResponse<Self, Result<UserConnection>>;

    fn handle(&mut self, msg: Connect, _: &mut Self::Context) -> Self::Result {
        let remote = match to_ip(&msg.address) {
            Ok(ip) => IpEndpoint::new(ip.into(), msg.port),
            Err(err) => return ActorResponse::reply(Err(err)),
        };

        log::info!("VPN {}: connecting to {:?}", self.vpn.id(), remote);

        let id = self.vpn.id().clone();
        let network = self.stack_network.clone();

        let fut = async move { network.connect(remote, TCP_CONN_TIMEOUT).await }
            .into_actor(self)
            .map(move |result, this, ctx| {
                let stack_connection = result?;
                log::info!("VPN {}: connected to {:?}", id, remote);
                let vpn = ctx.address().recipient();

                let (tx, rx) = mpsc::channel(1);

                this.connections.insert(
                    stack_connection.meta.into(),
                    InternalConnection {
                        stack_connection,
                        ingress_tx: tx,
                    },
                );

                Ok(UserConnection {
                    vpn,
                    rx,
                    stack_connection,
                })
            });

        ActorResponse::r#async(fut)
    }
}

impl Handler<Disconnect> for Vpn {
    type Result = <Disconnect as Message>::Result;

    fn handle(&mut self, msg: Disconnect, _: &mut Self::Context) -> Self::Result {
        match self.connections.remove(&msg.desc) {
            Some(mut connection) => {
                log::info!(
                    "Dropping connection to {:?}: {:?}",
                    msg.desc.remote,
                    msg.reason
                );

                connection.ingress_tx.close_channel();

                self.stack_network
                    .stack
                    .disconnect(connection.stack_connection.handle);

                Ok(())
            }
            None => Err(Error::ConnectionError(format!(
                "no connection to remote: {:?}",
                msg.desc.remote
            ))),
        }
    }
}

/// Handle egress packet from the user
impl Handler<Packet> for Vpn {
    type Result = ActorResponse<Self, Result<()>>;

    fn handle(&mut self, pkt: Packet, _: &mut Self::Context) -> Self::Result {
        match self.connections.get(&pkt.meta.into()) {
            Some(connection) => {
                let fut = self
                    .stack_network
                    .send(pkt.data, connection.stack_connection)
                    .map_err(|e| Error::Other(e.to_string()));
                ActorResponse::r#async(fut.into_actor(self))
            }
            None => ActorResponse::reply(Err(Error::ConnectionError(format!(
                "no connection to remote: {:?}",
                pkt.meta.remote
            )))),
        }
    }
}

/// Handle ingress packet from the network
impl Handler<RpcEnvelope<VpnPacket>> for Vpn {
    type Result = <RpcEnvelope<VpnPacket> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<VpnPacket>, _: &mut Self::Context) -> Self::Result {
        self.stack_network.receive(msg.into_inner().0);
        self.stack_network.poll();
        Ok(())
    }
}

impl Handler<RpcRawCall> for Vpn {
    type Result = std::result::Result<Vec<u8>, ya_service_bus::Error>;

    fn handle(&mut self, msg: RpcRawCall, _: &mut Self::Context) -> Self::Result {
        self.stack_network.receive(msg.body);
        self.stack_network.poll();
        Ok(Vec::new())
    }
}

/// Handle ingress event from stack
impl Handler<Ingress> for Vpn {
    type Result = ActorResponse<Self, Result<()>>;

    fn handle(&mut self, msg: Ingress, ctx: &mut Self::Context) -> Self::Result {
        match msg.event {
            IngressEvent::InboundConnection { desc } => {
                log::debug!(
                    "[vpn] ingress: connection to {:?} ({}) from {:?}",
                    desc.local,
                    desc.protocol,
                    desc.remote
                );
                ActorResponse::reply(Ok(()))
            }
            IngressEvent::Disconnected { desc } => {
                log::debug!(
                    "[vpn] ingress: disconnect {:?} ({}) by {:?}",
                    desc.local,
                    desc.protocol,
                    desc.remote,
                );
                ctx.address()
                    .do_send(Disconnect::new(desc, DisconnectReason::SocketClosed));

                ActorResponse::reply(Ok(()))
            }
            IngressEvent::Packet { payload, desc, .. } => {
                if let Some(mut connection) = self.connections.get(&desc).cloned() {
                    log::debug!("[vpn] ingress proxy: send to {:?}", desc.local);

                    let fut = async move { connection.ingress_tx.send(payload).await }
                        .into_actor(self)
                        .map(move |res, _, ctx| {
                            res.map_err(|e| {
                                ctx.address()
                                    .do_send(Disconnect::new(desc, DisconnectReason::SinkClosed));

                                Error::Other(e.to_string())
                            })
                        });
                    ActorResponse::r#async(fut)
                } else {
                    log::debug!("[vpn] ingress proxy: no connection to {:?}", desc);
                    ActorResponse::reply(Ok(()))
                }
            }
        }
    }
}

/// Handle egress event from stack
impl Handler<Egress> for Vpn {
    type Result = ActorResponse<Self, Result<()>>;

    fn handle(&mut self, msg: Egress, _: &mut Self::Context) -> Self::Result {
        let frame = msg.event.payload.into_vec();

        log::debug!("[vpn] egress -> runtime packet {} B", frame.len());

        match self.vpn.endpoint(msg.event.remote) {
            Some(endpoint) => {
                let fut = endpoint
                    .udp
                    .push_raw_as(&self.node_id, frame)
                    .map(|r| match r {
                        Ok(_) => Ok(()),
                        Err(e) => Err(Error::Other(e.to_string())),
                    });

                ActorResponse::r#async(fut.into_actor(self))
            }
            None => {
                log::debug!("No endpoint for egress packet");
                ActorResponse::reply(Ok(()))
            }
        }
    }
}

impl Handler<Shutdown> for Vpn {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct InternalConnection {
    pub stack_connection: stack::Connection,
    pub ingress_tx: mpsc::Sender<Vec<u8>>,
}

async fn vpn_ingress_handler(rx: IngressReceiver, addr: Addr<Vpn>, vpn_id: String) {
    let mut rx = UnboundedReceiverStream::new(rx);
    while let Some(event) = rx.next().await {
        addr.do_send(Ingress { event });
    }

    log::warn!("[vpn: {}] ingress handler stopped", vpn_id);
}

async fn vpn_egress_handler(rx: EgressReceiver, addr: Addr<Vpn>, vpn_id: String) {
    let mut rx = UnboundedReceiverStream::new(rx);
    while let Some(event) = rx.next().await {
        addr.do_send(Egress { event });
    }

    log::warn!("[vpn: {}] egress handler stopped", vpn_id);
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

fn gsb_remote_url(node_id: &str, net_id: &str) -> network::DuoEndpoint<Endpoint> {
    network::DuoEndpoint {
        tcp: typed::service(format!("/net/{}/vpn/{}", node_id, net_id)),
        udp: typed::service(format!("/udp/net/{}/vpn/{}/raw", node_id, net_id)),
    }
}

trait ArbiterExt {
    fn spawn_ext<'a, F, T, E>(&self, f: F) -> BoxFuture<'a, std::result::Result<T, E>>
    where
        F: Future<Output = std::result::Result<T, E>> + Send + 'static,
        T: Send + 'static,
        E: Send + From<Canceled> + 'static;
}

impl ArbiterExt for Arbiter {
    fn spawn_ext<'a, F, T, E>(&self, f: F) -> BoxFuture<'a, std::result::Result<T, E>>
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

        self.spawn(tx_fut);
        Box::pin(rx_fut)
    }
}

fn create_ethernet_addr(ip: IpCidr) -> Result<EthernetAddress> {
    match ip.address() {
        IpAddress::Ipv4(ip4) => Ok(EthernetAddress([
            0xA0, 0x13, ip4.0[0], ip4.0[1], ip4.0[2], ip4.0[3],
        ])),
        IpAddress::Ipv6(ip6) => Ok(EthernetAddress([
            0xA0, 0x13, ip6.0[12], ip6.0[13], ip6.0[14], ip6.0[15],
        ])),
        _ => Err(Error::Other(format!(
            "Could not create ethernet addr from ip: {:?}",
            ip
        ))),
    }
}

fn create_stack_network(
    node_ip: IpCidr,
    network_ip: IpCidr,
    network_gateway: IpAddr,
) -> Result<net::Network> {
    let config = Rc::new(StackConfig {
        max_transmission_unit: DEFAULT_MAX_PACKET_SIZE,
        ..Default::default()
    });

    let ethernet_addr = create_ethernet_addr(node_ip)?;

    let mut iface = tap_iface(
        HardwareAddress::Ethernet(ethernet_addr),
        config.max_transmission_unit,
    );

    add_iface_address(&mut iface, node_ip);
    add_iface_route(&mut iface, network_ip, net_route(network_gateway)?);

    let stack = net::Stack::new(iface, config.clone());

    Ok(net::Network::new("vpn", config, stack))
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
                node_id,
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
