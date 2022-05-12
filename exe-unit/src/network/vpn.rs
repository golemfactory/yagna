use std::convert::TryFrom;
use std::ops::Not;

use actix::prelude::*;
use futures::{future, FutureExt, SinkExt, TryFutureExt};

use ya_core_model::activity;
use ya_core_model::activity::{RpcMessageError, VpnControl, VpnPacket};
use ya_runtime_api::server::{CreateNetwork, NetworkInterface, RuntimeService};
use ya_service_bus::typed::Endpoint as GsbEndpoint;
use ya_service_bus::{actix_rpc, typed, RpcEnvelope};
use ya_utils_networking::vpn::network::DuoEndpoint;
use ya_utils_networking::vpn::{common::ntoh, Error as NetError, PeekPacket};
use ya_utils_networking::vpn::{ArpField, ArpPacket, EtherFrame, EtherType, IpPacket, Networks};

use crate::acl::Acl;
use crate::error::Error;
use crate::message::Shutdown;
use crate::network;
use crate::network::{Endpoint, RxBuffer};
use crate::state::Deployment;

pub(crate) async fn start_vpn<R: RuntimeService>(
    acl: Acl,
    service: &R,
    deployment: &Deployment,
) -> crate::Result<Option<Addr<Vpn>>> {
    if !deployment.networking() {
        return Ok(None);
    }

    log::info!("Starting VPN service...");

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
        .map_err(|e| Error::Other(format!("[vpn] initialization error: {:?}", e)))?;

    let endpoint = match response.endpoint {
        Some(endpoint) => Endpoint::connect(endpoint).await?,
        None => return Err(Error::Other("[vpn] endpoint already connected".into())),
    };

    let vpn = Vpn::try_new(acl, endpoint, deployment.clone())?;
    Ok(Some(vpn.start()))
}

pub(crate) struct Vpn {
    // TODO: Populate & use ACL
    #[allow(unused)]
    acl: Acl,
    networks: Networks<DuoEndpoint<GsbEndpoint>>,
    endpoint: Endpoint,
    rx_buf: Option<RxBuffer>,
}

impl Vpn {
    fn try_new(acl: Acl, endpoint: Endpoint, deployment: Deployment) -> crate::Result<Self> {
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
            acl,
            networks,
            endpoint,
            rx_buf: Some(Default::default()),
        })
    }

    fn handle_ip(&mut self, frame: EtherFrame, ctx: &mut Context<Self>) {
        let ip_pkt = IpPacket::packet(frame.payload());
        log::trace!("[vpn] egress packet to {:?}", ip_pkt.dst_address());

        if ip_pkt.is_broadcast() {
            let futs = self
                .networks
                .endpoints()
                .into_iter()
                .map(|e| e.udp.call(VpnPacket(frame.as_ref().to_vec())))
                .collect::<Vec<_>>();
            futs.is_empty().not().then(|| {
                let fut = future::join_all(futs).then(|_| future::ready(()));
                ctx.spawn(fut.into_actor(self))
            });
        } else {
            let ip = ip_pkt.dst_address();
            match self.networks.endpoint(ip) {
                Some(endpoint) => self.forward_frame(endpoint, frame, ctx),
                None => log::debug!("[vpn] no endpoint for {ip:?}"),
            }
        }
    }

    fn handle_arp(&mut self, frame: EtherFrame, ctx: &mut Context<Self>) {
        let arp = ArpPacket::packet(frame.payload());
        // forward only IP ARP packets
        if arp.get_field(ArpField::PTYPE) != [08, 00] {
            return;
        }

        let ip = arp.get_field(ArpField::TPA);
        match self.networks.endpoint(ip) {
            Some(endpoint) => self.forward_frame(endpoint, frame, ctx),
            None => log::debug!("[vpn] no endpoint for {ip:?}"),
        }
    }

    fn forward_frame(
        &mut self,
        endpoint: DuoEndpoint<GsbEndpoint>,
        frame: EtherFrame,
        ctx: &mut Context<Self>,
    ) {
        let pkt: Vec<_> = frame.into();
        log::trace!("[vpn] egress {} b", pkt.len());

        endpoint
            .udp
            .call(VpnPacket(pkt))
            .map_err(|err| log::debug!("[vpn] call error: {err}"))
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

            actix_rpc::bind::<VpnControl>(&vpn_id, ctx.address().recipient());
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
    fn handle(&mut self, result: crate::Result<Vec<u8>>, ctx: &mut Context<Self>) {
        let received = match result {
            Ok(vec) => vec,
            Err(err) => return log::debug!("[vpn] error (egress): {err}"),
        };
        let mut rx_buf = match self.rx_buf.take() {
            Some(buf) => buf,
            None => return log::error!("[vpn] programming error: rx buffer already taken"),
        };

        for packet in rx_buf.process(received) {
            match EtherFrame::try_from(packet) {
                Ok(frame) => match &frame {
                    EtherFrame::Arp(_) => self.handle_arp(frame, ctx),
                    EtherFrame::Ip(_) => self.handle_ip(frame, ctx),
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
        }

        self.rx_buf.replace(rx_buf);
    }
}

/// Ingress traffic handler (VPN -> Runtime)
impl Handler<Packet> for Vpn {
    type Result = <Packet as Message>::Result;

    fn handle(&mut self, packet: Packet, ctx: &mut Context<Self>) -> Self::Result {
        log::trace!("[vpn] ingress {} b", packet.data.len());

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
                let _ = self.networks.get_mut(&network_id).map(|network| {
                    if !network.nodes().contains_key(&node_id) {
                        log::debug!("[vpn] adding new node: {} {}", ip, node_id);
                        let _ = network.add_node(ip, &node_id, network::gsb_endpoint);
                    }
                });
            }
        }

        let mut data = data.into();
        network::write_prefix(&mut data);

        let mut tx = self.endpoint.tx.clone();
        async move {
            if let Err(e) = tx.send(Ok(data)).await {
                log::debug!("[vpn] ingress error: {}", e);
            }
        }
        .into_actor(self)
        .spawn(ctx);

        Ok(())
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
