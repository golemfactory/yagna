use std::collections::{BTreeSet, HashMap};
use std::convert::TryFrom;
use std::net::IpAddr;
use std::ops::DerefMut;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use actix::prelude::*;
use futures::channel::oneshot::Canceled;
use futures::channel::{mpsc, oneshot};
use futures::{future, future::BoxFuture, Future, FutureExt, SinkExt, StreamExt, TryFutureExt};
use smoltcp::iface::{Route, SocketHandle};
use smoltcp::socket::Socket;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint};
use tokio::sync::RwLock;
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

use ya_utils_networking::vpn::socket::{SocketExt, TCP_CONN_TIMEOUT};
use ya_utils_networking::vpn::stack::connection::{Connection as Dupa, ConnectionMeta};
use ya_utils_networking::vpn::stack::interface::{add_iface_route, tap_iface};

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

const STACK_POLL_INTERVAL: Duration = Duration::from_millis(2500);
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
                let vpn = Vpn::new(node_id, vpn_net);
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
    vpn: Arc<std::sync::RwLock<Network<network::DuoEndpoint<Endpoint>>>>,
    network: net::Network,
    ingress_senders: Arc<RwLock<HashMap<SocketDesc, mpsc::Sender<Vec<u8>>>>>,
}

impl Vpn {
    pub fn new(node_id: NodeId, vpn: Network<network::DuoEndpoint<Endpoint>>) -> Self {
        Self {
            node_id: node_id.to_string(),
            vpn: Arc::new(std::sync::RwLock::new(vpn)),
            network: Self::create_network(),
            ingress_senders: Arc::new(RwLock::new(Default::default())),
        }
    }

    fn create_network() -> net::Network {
        let config = Rc::new(StackConfig {
            max_transmission_unit: DEFAULT_MAX_PACKET_SIZE,
            ..Default::default()
        });

        let ethernet_addr = loop {
            let addr = EthernetAddress(rand::random());
            if addr.is_unicast() {
                break addr;
            }
        };

        let mut iface = tap_iface(
            HardwareAddress::Ethernet(ethernet_addr),
            config.max_transmission_unit,
        );

        // add_iface_route(&mut iface, net_ip, net_route(net_gw)?); //TODO Rafał is necessary?

        let stack = net::Stack::new(iface, config.clone());

        // stack.add_address(IpCidr::new(IP4_ADDRESS.into(), 16));
        // stack.add_address(IpCidr::new(IP6_ADDRESS.into(), 0));

        net::Network::new("vpn", config, stack) //TODO Rafał name?
    }
}

impl Actor for Vpn {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let vpn = self.vpn.read().expect("dupa");
        let id = vpn.id();
        let vpn_url = gsb_local_url(id);
        let addr = ctx.address();
        self.network.spawn_local(); //TODO Rafał env is there

        actix_rpc::bind(&vpn_url, addr.clone().recipient());
        actix_rpc::bind_raw(&format!("{vpn_url}/raw"), addr.recipient());

        // ctx.run_interval(STACK_POLL_INTERVAL, |this, ctx| {
        //     this.poll(ctx.address());
        // });

        let ingress_rx = self
            .network
            .ingress_receiver()
            .expect("Ingress receiver already taken");

        let egress_rx = self
            .network
            .egress_receiver()
            .expect("Egress receiver already taken");

        //TODO Rafał other handlers

        //     inet_endpoint_egress_handler(endpoint_rx, router)
        //     .into_actor(self)
        //     .spawn(ctx);

        vpn_ingress_handler(ingress_rx, self.ingress_senders.clone())
            .into_actor(self)
            .spawn(ctx);

        vpn_egress_handler(egress_rx, self.vpn.clone(), self.node_id.clone())
            .into_actor(self)
            .spawn(ctx);

        log::info!("VPN {} started", id);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        //TODO Rafał proxy?
        log::warn!("Stopping VPN {}", self.vpn.read().expect("dupa").id());
        Running::Stop
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        //TODO Rafał is it needed?
        let id = self.vpn.read().expect("dupa").id().clone();
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
            .network
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

        let vpn = self.vpn.write().expect("dupa");
        let net = vpn.as_ref();
        if !net.contains(&ip) {
            return Err(Error::NetAddrMismatch(ip));
        }

        let cidr = IpCidr::new(IpAddress::from(ip), net.prefix_len());
        if !cidr.address().is_unicast() && !cidr.address().is_unspecified() {
            return Err(Error::IpAddrNotAllowed(ip));
        }

        self.network.stack.add_address(cidr);
        self.vpn.write().expect("dupa").add_address(&msg.address)?;

        Ok(())
    }
}

impl Handler<GetNodes> for Vpn {
    type Result = <GetNodes as Message>::Result;

    fn handle(&mut self, _: GetNodes, _: &mut Self::Context) -> Self::Result {
        Ok(self
            .vpn
            .read()
            .expect("dupa")
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
        match self
            .vpn
            .write()
            .expect("dupa")
            .add_node(ip, &msg.id, gsb_remote_url)
        {
            Ok(_) | Err(Error::IpAddrTaken(_)) => {}
            Err(err) => return Err(err),
        }

        let vpn_id = self.vpn.read().expect("dupa").id().clone();
        let futs = self
            .vpn
            .read()
            .expect("dupa")
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
        self.vpn.write().expect("dupa").remove_node(&msg.id);

        let vpn_id = self.vpn.read().expect("dupa").id().clone();
        let futs = self
            .vpn
            .read()
            .expect("dupa")
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

    fn handle(&mut self, msg: Connect, ctx: &mut Self::Context) -> Self::Result {
        let remote = match to_ip(&msg.address) {
            Ok(ip) => IpEndpoint::new(ip.into(), msg.port),
            Err(err) => return ActorResponse::reply(Err(err)),
        };

        log::info!(
            "VPN {}: connecting to {:?}",
            self.vpn.read().expect("dupa").id(),
            remote
        );
        //TODO Rafał think about UDP later (bind some port?)

        //TODO Rafał Shady AF
        let id = self.vpn.read().expect("dupa").id().clone();
        let network = self.network.clone();
        let ingress_senders = self.ingress_senders.clone();
        let vpn = ctx.address().recipient();

        let fut = async move {
            let connection = network.connect(remote, TCP_CONN_TIMEOUT).await?;

            log::info!("VPN {}: connected to {:?}", id, remote);

            let (tx, rx) = mpsc::channel(1);

            ingress_senders
                .write()
                .await
                .insert(connection.meta.clone().into(), tx);

            Ok(UserConnection {
                vpn,
                rx,
                connection,
            })
        };

        ActorResponse::r#async(fut.into_actor(self))
    }
}

/// Handle egress packet from the user
impl Handler<Packet> for Vpn {
    type Result = ActorResponse<Self, Result<()>>;

    fn handle(&mut self, pkt: Packet, ctx: &mut Self::Context) -> Self::Result {
        //TODO Rafał make public? + incompatibility
        // if !self.connections.contains_key(&pkt.meta.handle) {
        //     return ActorResponse::reply(Err(Error::ConnectionError("no connection".into())));
        // }
        let addr = ctx.address();
        let fut = self
            .network
            .send(pkt.data, pkt.connection)
            .map_err(|e| Error::Other(e.to_string()));
        ActorResponse::r#async(fut.into_actor(self))
    }
}

/// Handle ingress packet from the network
impl Handler<RpcEnvelope<VpnPacket>> for Vpn {
    type Result = <RpcEnvelope<VpnPacket> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<VpnPacket>, ctx: &mut Self::Context) -> Self::Result {
        self.network.receive(msg.into_inner().0);
        self.network.poll();
        Ok(())
    }
}

impl Handler<RpcRawCall> for Vpn {
    type Result = std::result::Result<Vec<u8>, ya_service_bus::Error>;

    fn handle(&mut self, msg: RpcRawCall, ctx: &mut Self::Context) -> Self::Result {
        self.network.receive(msg.body);
        self.network.poll();
        Ok(Vec::new())
    }
}

impl Handler<Shutdown> for Vpn {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}

async fn vpn_ingress_handler(
    rx: IngressReceiver,
    ingress_senders: Arc<RwLock<HashMap<SocketDesc, mpsc::Sender<Vec<u8>>>>>,
) {
    let mut rx = UnboundedReceiverStream::new(rx);
    while let Some(event) = rx.next().await {
        match event {
            IngressEvent::InboundConnection { desc } => log::debug!(
                "[vpn] ingress: connection to {:?} ({}) from {:?}",
                desc.local,
                desc.protocol,
                desc.remote
            ),
            IngressEvent::Disconnected { desc } => {
                log::debug!(
                    "[vpn] ingress: disconnect {:?} ({}) by {:?}",
                    desc.local,
                    desc.protocol,
                    desc.remote,
                );
                // let _ = proxy.unbind(desc).await;
            }
            IngressEvent::Packet { payload, desc, .. } => {
                if let Some(mut sender) = ingress_senders.read().await.get(&desc).cloned() {
                    log::debug!("[vpn] ingress proxy: send to {:?}", desc.local);

                    if let Err(e) = sender.send(payload).await {
                        log::debug!("[vpn] ingress proxy: send error: {}", e);
                    }
                } else {
                    log::debug!("[vpn] ingress proxy: no connection to {:?}", desc);
                }
            }
        }
    }

    log::debug!("[vpn] ingress handler stopped");
}

//TODO Rafał send to VPN
async fn vpn_egress_handler(
    rx: EgressReceiver,
    vpn: Arc<std::sync::RwLock<Network<network::DuoEndpoint<Endpoint>>>>,
    node_id: String,
) {
    let mut rx = UnboundedReceiverStream::new(rx);
    while let Some(event) = rx.next().await {
        let frame = event.payload.into_vec();

        log::debug!("[vpn] egress -> runtime packet {} B", frame.len());

        let endpoint = match vpn.read().expect("dupa").endpoint(event.remote) {
            Some(endpoint) => endpoint,
            None => {
                log::trace!("No endpoint for egress packet");
                continue;
            }
        };

        endpoint.udp.push_raw_as(&node_id, frame);
    }

    log::debug!("[vpn] egress -> runtime handler stopped");
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
