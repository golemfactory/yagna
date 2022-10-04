use std::convert::TryFrom;
use std::process::Stdio;
use std::time::Duration;

use actix::prelude::*;
use futures::{future, FutureExt};
use ipnet::IpNet;
use tokio::net::UnixDatagram;

use ya_client_model::NodeId;
use ya_core_model::activity::{self, RpcMessageError, VpnControl, VpnPacket};
use ya_core_model::identity;
use ya_runtime_api::server::{CreateNetwork, NetworkInterface, RuntimeService};
use ya_service_bus::typed::Endpoint as GsbEndpoint;
use ya_service_bus::{actix_rpc, typed, RpcEndpoint, RpcEnvelope, RpcRawCall};
use ya_utils_networking::vpn::network::DuoEndpoint;
use ya_utils_networking::vpn::{common::ntoh, Error as NetError, PeekPacket};
use ya_utils_networking::vpn::{ArpField, ArpPacket, EtherFrame, EtherType, IpPacket, Networks};

use crate::acl::Acl;
use crate::error::Error;
use crate::message::Shutdown;
use crate::network;
use crate::network::{Endpoint, RxBuffer};
use crate::state::Deployment;

async fn endpoint_hack(
    ip_addr: std::net::IpAddr,
    endpoint: impl Into<ya_runtime_api::deploy::ContainerEndpoint>,
) -> crate::network::Result<(Endpoint, String)> {
    let socket_name = "tap0";
    let socket_dir = std::env::temp_dir().join(ip_addr.to_string());

    let _ = std::fs::remove_dir_all(&socket_dir);
    std::fs::create_dir_all(&socket_dir).map_err(|e| {
        Error::Other(format!(
            "unable to create temp dir {}: {}",
            socket_dir.display(),
            e
        ))
    })?;

    let net = IpNet::new(ip_addr, 24).map_err(|e| Error::Other(e.to_string()))?;
    let gw_addr = net.hosts().next().ok_or_else(|| {
        Error::Other("No host addresses are available in the network".to_string())
    })?;

    let env_ip_addr = format!("IP_ADDR={ip_addr}");
    let env_gw_ip_addr = format!("IP_GW={gw_addr}");
    let env_tap_name = format!("TAP_NAME={socket_name}");
    let volume = format!("{}:/golem/output", socket_dir.display());

    let child = tokio::process::Command::new("/usr/bin/docker")
        .envs(std::env::vars())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .arg("run")
        .arg("-d")
        .arg("--rm")
        .arg("--privileged")
        .arg("--env")
        .arg(env_ip_addr.as_str())
        .arg("--env")
        .arg(env_tap_name.as_str())
        .arg("--env")
        .arg(env_gw_ip_addr.as_str())
        .arg("-v")
        .arg(volume.as_str())
        .arg("docker-tap")
        .spawn()?;

    log::info!("docker run --rm --privileged --env {env_ip_addr} --env {env_tap_name} -v {volume} docker-tap");

    let bound_path = socket_dir.join(socket_name);
    let connect_path = socket_dir.join(format!("{}-write", socket_name));

    log::info!("binding socket at {}", bound_path.display());

    let _ = tokio::fs::remove_file(bound_path.as_path()).await;

    // let bound = UnixDatagram::bind(bound_path.as_path())?;
    // let connected = UnixDatagram::unbound()?;

    let bound = UnixDatagram::bind(bound_path.as_path())?;
    let connected = UnixDatagram::unbound()?;

    let _endpoint = endpoint.into();
    let (tx, rx) = futures::channel::oneshot::channel();

    tokio::task::spawn_local(async move {
        log::info!("spawning network docker container");

        match child.wait_with_output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                log::info!("network docker container status: {}", output.status);
                log::info!("stdout: {}", stdout);
                log::info!("stderr: {}", stderr);

                tx.send(Ok(stdout.to_string())).unwrap();
            }
            Err(e) => {
                log::error!("network docker error: {e}");

                tx.send(Err(Error::Other(e.to_string()))).unwrap();
            }
        }
    });

    let container = rx.await.unwrap()?.as_str()[..12].to_string();

    log::info!("container: {container}");
    log::info!(
        "expecting network docker container socket at: {}",
        connect_path.display()
    );

    let path_ = connect_path.clone();
    tokio::time::timeout(Duration::from_secs(10), async move {
        while !path_.exists() {
            tokio::time::sleep(Duration::from_millis(250)).await;
            match std::fs::read_dir(path_.parent().unwrap()) {
                Ok(paths) => {
                    for path in paths.flatten() {
                        log::info!("Name: {}", path.path().display());
                    }
                }
                Err(e) => {
                    log::error!("unable to read parent dir of {}: {e}", path_.display());
                }
            }
        }
    })
    .await
    .map_err(|_| {
        Error::RuntimeError(format!(
            "timeout setting up the network docker container: {}",
            connect_path.display()
        ))
    })?;

    connected.connect(connect_path.as_path()).map_err(|e| {
        Error::Other(format!(
            "unable to connect to socket at {}: {e}",
            connect_path.display()
        ))
    })?;

    let endpoint = Endpoint::connect_with((bound, connected)).await?;
    Ok((endpoint, container))
}

pub(crate) async fn start_vpn<R: RuntimeService>(
    acl: Acl,
    service: &R,
    deployment: &Deployment,
) -> crate::Result<Option<(Addr<Vpn>, String)>> {
    if !deployment.networking() {
        return Ok(None);
    }

    log::info!("Starting VPN service...");

    let node_id = typed::service(identity::BUS_ID)
        .send(identity::Get::ByDefault)
        .await?
        .map_err(|e| Error::Other(format!("failed to retrieve default identity: {e}")))?
        .ok_or_else(|| Error::Other("no default identity set".to_string()))?
        .node_id;

    let ip_addr = deployment
        .networks
        .values()
        .next()
        .map(|n| n.node_ip)
        .ok_or_else(|| Error::Other("no ip address set".to_string()))?;
    let networks = deployment
        .networks
        .values()
        .map(TryFrom::try_from)
        .collect::<crate::Result<_>>()?;

    let hosts = deployment.hosts.clone();
    let response = service
        .create_network(CreateNetwork {
            networks,
            hosts,
            interface: NetworkInterface::Vpn as i32,
        })
        .await
        .map_err(|e| Error::Other(format!("initialization error: {:?}", e)))?;

    let (endpoint, container) = match response.endpoint {
        Some(endpoint) => endpoint_hack(ip_addr, endpoint).await?,
        None => return Err(Error::Other("endpoint already connected".into())),
    };

    let vpn = Vpn::try_new(node_id, acl, endpoint, deployment.clone())?;
    Ok(Some((vpn.start(), container)))
}

pub(crate) struct Vpn {
    default_id: String,
    // TODO: Populate & use ACL
    #[allow(unused)]
    acl: Acl,
    networks: Networks<DuoEndpoint<GsbEndpoint>>,
    endpoint: Endpoint,
    rx_buf: RxBuffer,
    is_tap: bool,
}

impl Vpn {
    fn try_new(
        node_id: NodeId,
        acl: Acl,
        endpoint: Endpoint,
        deployment: Deployment,
    ) -> crate::Result<Self> {
        let mut networks = Networks::default();

        deployment
            .networks
            .iter()
            .try_for_each(|(id, net)| networks.add(id.clone(), net.network))?;

        deployment.networks.into_iter().try_for_each(|(id, net)| {
            let network = networks.get_mut(&id).unwrap();
            net.nodes
                .into_iter()
                .try_for_each(|(ip, id)| network.add_node(ip, &id, network::gsb_endpoint))?;
            Ok::<_, NetError>(())
        })?;

        Ok(Self {
            default_id: node_id.to_string(),
            acl,
            networks,
            endpoint,
            rx_buf: Default::default(),
            is_tap: true,
        })
    }

    fn handle_packet(
        &mut self,
        packet: Packet,
        _ctx: &mut Context<Self>,
    ) -> <Packet as Message>::Result {
        let network_id = packet.network_id;
        let node_id = packet.caller;
        let mut data = packet.data;

        // log::info!("receive packet {} B", data.len());
        // {
        //     let d = DbgVec(&data);
        //     log::info!("{:x}", d);
        // }

        // fixme: should requestor be queried for unknown IP addresses instead?
        let ip = if self.is_tap {
            match EtherFrame::peek_type(&data) {
                Ok(ether_type) => {
                    let payload = EtherFrame::peek_payload(&data).unwrap();
                    match ether_type {
                        EtherType::Arp => {
                            let pkt = ArpPacket::packet(payload);
                            ntoh(pkt.get_field(ArpField::SPA))
                        }
                        EtherType::Ip => {
                            let pkt = IpPacket::packet(payload);
                            ntoh(pkt.src_address())
                        }
                        _ => None,
                    }
                }
                _ => None,
            }
        } else {
            match IpPacket::peek(&data) {
                Ok(_) => {
                    let pkt = IpPacket::packet(&data);
                    ntoh(pkt.src_address())
                }
                _ => None,
            }
        };

        if let Some(ip) = ip {
            let _ = self.networks.get_mut(&network_id).map(|network| {
                if !network.nodes().contains_key(&node_id) {
                    log::debug!("[vpn] adding new node: {} {}", ip, node_id);
                    let _ = network.add_node(ip, &node_id, network::gsb_endpoint);
                }
            });
        }

        network::write_prefix(&mut data);

        if let Err(e) = self.endpoint.tx.send(Ok(data)) {
            log::debug!("[vpn] ingress error: {}", e);
        }

        Ok(())
    }

    fn handle_eth_ip(
        frame: EtherFrame,
        networks: &Networks<DuoEndpoint<GsbEndpoint>>,
        default_id: &str,
    ) {
        let ip_pkt = IpPacket::packet(frame.payload());
        log::trace!("[vpn] egress packet to {:?}", ip_pkt.dst_address());

        if ip_pkt.is_broadcast() {
            let futs = networks
                .endpoints()
                .into_iter()
                .map(|e| e.udp.push_raw_as(default_id, frame.as_ref().to_vec()))
                .collect::<Vec<_>>();
            tokio::task::spawn_local(async move {
                future::join_all(futs).then(|_| future::ready(())).await;
            });
        } else {
            let ip = ip_pkt.dst_address();
            match networks.endpoint(ip) {
                Some(endpoint) => Self::endpoint_send(endpoint, default_id, frame.into()),
                None => log::debug!("[vpn] no endpoint for {ip:?}"),
            }
        }
    }

    fn handle_eth_arp(
        frame: EtherFrame,
        networks: &Networks<DuoEndpoint<GsbEndpoint>>,
        default_id: &str,
    ) {
        let arp = ArpPacket::packet(frame.payload());
        // forward only IP ARP packets
        if arp.get_field(ArpField::PTYPE) != [08, 00] {
            return;
        }

        let ip = arp.get_field(ArpField::TPA);
        match networks.endpoint(ip) {
            Some(endpoint) => Self::endpoint_send(endpoint, default_id, frame.into()),
            None => log::debug!("[vpn] no endpoint for {ip:?}"),
        }
    }

    fn handle_ip(data: Vec<u8>, networks: &Networks<DuoEndpoint<GsbEndpoint>>, default_id: &str) {
        let ip_pkt = IpPacket::packet(data.as_slice());
        // log::debug!("[vpn] egress packet to {:?}", ip_pkt.dst_address());

        let ip = ip_pkt.dst_address();
        match networks.endpoint(ip) {
            Some(endpoint) => {
                drop(ip_pkt);
                Self::endpoint_send(endpoint, default_id, data)
            }
            None => log::debug!("[vpn] no endpoint for {ip:?}"),
        }
    }

    fn endpoint_send(endpoint: DuoEndpoint<GsbEndpoint>, default_id: &str, data: Vec<u8>) {
        // {
        //     let d = DbgVec(&data);
        //     log::info!("{:x}", d);
        // }

        // log::info!("send packet {} B", data.len());

        let fut = endpoint
            .udp
            .push_raw_as(default_id, data)
            .then(|result| async move {
                if let Err(err) = result {
                    log::debug!("[vpn] call error: {err}");
                }
            });

        tokio::task::spawn_local(fut);
    }
}

struct DbgVec<'a>(&'a Vec<u8>);

impl<'a> std::fmt::LowerHex for DbgVec<'a> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        for byte in self.0 {
            fmt.write_fmt(format_args!("{:02x}", byte))?;
        }
        Ok(())
    }
}

impl Actor for Vpn {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.networks.as_ref().keys().for_each(|net| {
            let actor = ctx.address();
            let net_id = net.clone();
            let vpn_id = activity::exeunit::network_id(&net_id);

            actix_rpc::bind::<VpnControl>(&vpn_id, ctx.address().recipient());
            actix_rpc::bind_raw(&format!("{vpn_id}/raw"), ctx.address().recipient());

            typed::bind_with_caller::<VpnPacket, _, _>(&vpn_id, move |caller, pkt| {
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
                log::info!("[vpn] service started")
            }
            None => {
                log::error!("[vpn] local endpoint missing");
                ctx.stop();
            }
        };
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        Running::Continue
        // log::info!("[vpn] stopping service");
        //
        // let networks = self.networks.as_ref().keys().cloned().collect::<Vec<_>>();
        // async move {
        //     for net in networks {
        //         let vpn_id = activity::exeunit::network_id(&net);
        //         let _ = typed::unbind(&vpn_id).await;
        //     }
        // }
        // .into_actor(self)
        // .wait(ctx);
        //
        // Running::Stop
    }
}

/// Egress traffic handler (Runtime -> VPN)
impl StreamHandler<crate::Result<Vec<u8>>> for Vpn {
    fn handle(&mut self, result: crate::Result<Vec<u8>>, _ctx: &mut Context<Self>) {
        let received = match result {
            Ok(vec) => vec,
            Err(err) => return log::debug!("[vpn] error (egress): {err}"),
        };

        let networks = &self.networks;
        let rx_buf = &mut self.rx_buf;

        for packet in rx_buf.process(received) {
            if self.is_tap {
                match EtherFrame::try_from(packet) {
                    Ok(frame) => match &frame {
                        EtherFrame::Arp(_) => {
                            Self::handle_eth_arp(frame, networks, &self.default_id)
                        }
                        EtherFrame::Ip(_) => Self::handle_eth_ip(frame, networks, &self.default_id),
                        frame => log::debug!("[vpn] unimplemented EtherType: {}", frame),
                    },
                    Err(err) => {
                        match &err {
                            NetError::ProtocolNotSupported(_) => (),
                            _ => log::debug!("[vpn] frame error (egress): {}", err),
                        };
                        continue;
                    }
                };
            } else {
                Self::handle_ip(packet, networks, &self.default_id);
            }
        }
    }
}

/// Ingress traffic handler (VPN -> Runtime)
impl Handler<RpcRawCall> for Vpn {
    type Result = Result<Vec<u8>, ya_service_bus::Error>;

    fn handle(&mut self, msg: RpcRawCall, ctx: &mut Self::Context) -> Self::Result {
        let packet = {
            let mut split = msg.addr.rsplit('/').skip(1);
            match split.next() {
                Some(network_id) => Packet {
                    network_id: network_id.to_string(),
                    caller: msg.caller.to_string(),
                    data: msg.body,
                },
                None => {
                    return Err(ya_service_bus::Error::GsbBadRequest(
                        "Empty network id in a RpcRawCall message".to_string(),
                    ))
                }
            }
        };

        self.handle_packet(packet, ctx)
            .map(|_| Vec::new())
            .map_err(|e| ya_service_bus::Error::GsbBadRequest(e.to_string()))
    }
}

impl Handler<Packet> for Vpn {
    type Result = <Packet as Message>::Result;

    fn handle(&mut self, packet: Packet, ctx: &mut Context<Self>) -> Self::Result {
        self.handle_packet(packet, ctx)
    }
}

impl Handler<RpcEnvelope<VpnControl>> for Vpn {
    type Result = <RpcEnvelope<VpnControl> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<VpnControl>, _: &mut Context<Self>) -> Self::Result {
        // if !self.acl.has_access(msg.caller(), AccessRole::Control) {
        //     return Err(AclError::Forbidden(msg.caller().to_string(), AccessRole::Control).into());
        // }

        match msg.into_inner() {
            VpnControl::AddNodes { network_id, nodes } => {
                let network = self.networks.get_mut(&network_id).map_err(Error::from)?;
                for (ip, id) in Deployment::map_nodes(nodes).map_err(Error::from)? {
                    network
                        .add_node(ip, &id, network::gsb_endpoint)
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
        log::info!("[vpn] shutting down: {:?}", msg.0);
        ctx.stop();
        Ok(())
    }
}

#[derive(Message)]
#[rtype(result = "<RpcEnvelope<VpnPacket> as Message>::Result")]
pub(crate) struct Packet {
    pub network_id: String,
    pub caller: String,
    pub data: Vec<u8>,
}
