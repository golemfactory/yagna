use std::convert::{TryFrom, TryInto};

use actix::prelude::*;
use futures::{future, FutureExt};

use ya_client_model::NodeId;
use ya_core_model::activity::{self, RpcMessageError, VpnControl, VpnPacket};
use ya_core_model::identity;
use ya_runtime_api::deploy::ContainerEndpoint;
use ya_runtime_api::server::{CreateNetwork, NetworkInterface, RuntimeService};
use ya_service_bus::typed::Endpoint as GsbEndpoint;
use ya_service_bus::{actix_rpc, typed, RpcEndpoint, RpcEnvelope, RpcRawCall};
use ya_utils_networking::vpn::network::DuoEndpoint;
use ya_utils_networking::vpn::{common::ntoh, Error as NetError, EtherField, PeekPacket};
use ya_utils_networking::vpn::{ArpField, ArpPacket, EtherFrame, EtherType, IpPacket, Networks};

use crate::acl::Acl;
use crate::error::Error;
use crate::message::Shutdown;
use crate::network::{self, Endpoint};
use crate::state::Deployment;

pub(crate) async fn start_vpn<R: RuntimeService>(
    mut endpoint: Endpoint,
    acl: Acl,
    service: &R,
    deployment: &Deployment,
) -> crate::Result<Option<Addr<Vpn>>> {
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

    let networks = deployment
        .networks
        .values()
        .map(TryFrom::try_from)
        .collect::<crate::Result<_>>()?;

    let response = service
        .create_network(CreateNetwork {
            networks,
            hosts: deployment.hosts.clone(),
            interface: NetworkInterface::Vpn as i32,
        })
        .await
        .map_err(|e| Error::Other(format!("initialization error: {:?}", e)))?;

    match response.endpoint {
        Some(ep) => {
            let cep = ContainerEndpoint::try_from(&ep)
                .map_err(|e| Error::Other(format!("Invalid endpoint '{ep:?}': {e}")))?;
            endpoint.connect(cep).await?
        }
        None => {
            return Err(Error::Other(
                "No VM VPN network endpoint in CreateNetwork response".into(),
            ))
        }
    };

    let vpn = Vpn::try_new(node_id, acl, endpoint, deployment.clone())?;
    Ok(Some(vpn.start()))
}

pub(crate) struct Vpn {
    default_id: String,
    // TODO: Populate & utilize ACL
    #[allow(unused)]
    acl: Acl,
    networks: Networks<DuoEndpoint<GsbEndpoint>>,
    endpoint: Endpoint,
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
        })
    }

    fn handle_packet(
        &mut self,
        packet: Packet,
        _ctx: &mut Context<Self>,
    ) -> <Packet as Message>::Result {
        let network_id = packet.network_id;
        let node_id = packet.caller;
        let data = packet.data;

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
                let _ = self.networks.get_mut(&network_id).map(|network| {
                    if !network.nodes().contains_key(&node_id) {
                        log::debug!("[vpn] adding new node: {} {}", ip, node_id);
                        let _ = network.add_node(ip, &node_id, network::gsb_endpoint);
                    }
                });
            }
        }

        if let Err(e) = self.endpoint.send(Ok(data)) {
            log::debug!("[vpn] ingress error: {}", e);
        }

        Ok(())
    }

    fn handle_ip(
        dst_mac: [u8; 6],
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
                Some(endpoint) => Self::forward_frame(endpoint, default_id, frame),
                None => {
                    //yagna local network mac address assignment
                    if dst_mac[0] == 0xA0 && dst_mac[1] == 0x13 {
                        //last four bytes should be ip address (our convention of assigning mac addresses)
                        match networks.endpoint(&dst_mac[2..6]) {
                            Some(endpoint) => Self::forward_frame(endpoint, default_id, frame),
                            None => {
                                log::debug!("[vpn] endpoint not found {:?} or {:?}", &ip, &dst_mac[2..6])
                            },
                        }
                    } else {
                        log::debug!("[vpn] mac address not recognized {dst_mac:?}")
                    }
                },
            }
        }
    }

    fn handle_arp(
        frame: EtherFrame,
        networks: &Networks<DuoEndpoint<GsbEndpoint>>,
        default_id: &str,
    ) {
        let arp = ArpPacket::packet(frame.payload());
        // forward only IP ARP packets
        if arp.get_field(ArpField::PTYPE) != [8, 0] {
            return;
        }

        let ip = arp.get_field(ArpField::TPA);
        match networks.endpoint(ip) {
            Some(endpoint) => Self::forward_frame(endpoint, default_id, frame),
            None => log::debug!("[vpn] no endpoint for {ip:?}"),
        }
    }

    fn forward_frame(endpoint: DuoEndpoint<GsbEndpoint>, default_id: &str, frame: EtherFrame) {
        let data: Vec<_> = frame.into();
        log::trace!("[vpn] egress {} b to {}", data.len(), endpoint.udp.addr());

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

        match self.endpoint.receiver().ok() {
            Some(rx) => {
                Self::add_stream(rx, ctx);
                log::info!("[vpn] service started")
            }
            None => {
                log::error!("[vpn] VM endpoint already taken");
                ctx.stop();
            }
        };
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        log::info!("[vpn] stopping service");

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

/// Egress traffic handler (Runtime -> VPN)
impl StreamHandler<crate::Result<Vec<u8>>> for Vpn {
    fn handle(&mut self, result: crate::Result<Vec<u8>>, _ctx: &mut Context<Self>) {
        let packet = match result {
            Ok(vec) => vec,
            Err(err) => return log::debug!("[vpn] error (egress): {err}"),
        };

        ya_packet_trace::packet_trace_maybe!("exe-unit::Vpn::Handler<Egress>", {
            ya_packet_trace::try_extract_from_ip_frame(&packet)
        });

        if packet.len() < 14 {
            log::debug!("[vpn] packet too short (egress)");
            return;
        }
        let dst_mac: [u8; 6] = packet[EtherField::DST_MAC].try_into().unwrap();
        match EtherFrame::try_from(packet) {
            Ok(frame) => match &frame {
                EtherFrame::Arp(_) => Self::handle_arp(frame, &self.networks, &self.default_id),
                EtherFrame::Ip(_) => {
                    Self::handle_ip(dst_mac, frame, &self.networks, &self.default_id)
                },
                frame => log::debug!("[vpn] unimplemented EtherType: {}", frame),
            },
            Err(err) => {
                match &err {
                    NetError::ProtocolNotSupported(_) => (),
                    _ => log::debug!("[vpn] frame error (egress): {}", err),
                };
            }
        };
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

        ya_packet_trace::packet_trace_maybe!("exe-unit::Vpn::Handler<Ingress>", {
            &ya_packet_trace::try_extract_from_ip_frame(&packet.data)
        });

        self.handle_packet(packet, ctx)
            .map(|_| Vec::new())
            .map_err(|e| ya_service_bus::Error::GsbBadRequest(e.to_string()))
    }
}

impl Handler<Packet> for Vpn {
    type Result = <Packet as Message>::Result;

    fn handle(&mut self, packet: Packet, ctx: &mut Context<Self>) -> Self::Result {
        ya_packet_trace::packet_trace_maybe!("exe-unit::Vpn::Handler<Packet>", {
            &ya_packet_trace::try_extract_from_ip_frame(&packet.data)
        });

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
