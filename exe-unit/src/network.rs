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
use ya_core_model::activity::VpnControl;
use ya_runtime_api::server::{CreateNetwork, Network, NetworkEndpoint, RuntimeService};
use ya_service_bus::{actix_rpc, typed, typed::Endpoint as GsbEndpoint, RpcEnvelope};
use ya_utils_networking::vpn::error::Error as VpnError;
use ya_utils_networking::vpn::{
    self, ArpField, ArpPacket, EtherFrame, IpPacket, Networks, PeekPacket, MAX_FRAME_SIZE,
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
    acl: Acl,
    networks: Networks<GsbEndpoint>,
    endpoint: VpnEndpoint,
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
        // forward only for the IP protocol type
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
        log::debug!("Egress frame {:?}", pkt);

        ctx.spawn(
            endpoint
                .call(activity::VpnPacket(pkt))
                .map_err(|err| log::debug!("VPN call error: {}", err))
                .then(|_| future::ready(()))
                .into_actor(self),
        );
    }
}

impl Actor for Vpn {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.networks.as_ref().keys().for_each(|net| {
            let vpn_id = activity::exeunit::network_id(net);
            actix_rpc::bind::<activity::VpnControl>(&vpn_id, ctx.address().recipient());
            actix_rpc::bind::<activity::VpnPacket>(&vpn_id, ctx.address().recipient());
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
        ctx.wait(
            async move {
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
        log::debug!("Egress packet {:?}", result);

        let bytes = match result {
            Ok(bytes) => bytes,
            Err(err) => return log::debug!("VPN error (egress): {}", err),
        };
        let frame = match EtherFrame::try_from(bytes) {
            Ok(frame) => frame,
            Err(err) => match &err {
                VpnError::ProtocolNotSupported(_) => return,
                _ => return log::debug!("VPN frame error (egress): {}", err),
            },
        };
        match &frame {
            EtherFrame::Arp(_) => self.handle_arp(frame, ctx),
            EtherFrame::Ip(_) => self.handle_ip(frame, ctx),
            frame => log::debug!("VPN: unimplemented EtherType: {}", frame),
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
        let packet = packet.into_inner();
        let mut tx = self.endpoint.tx.clone();
        log::debug!(
            "Ingress packet of size {:?}: {:?}",
            packet.0.len(),
            packet.0
        );

        ctx.spawn(
            async move {
                if let Err(e) = tx.send(Ok(packet.0)).await {
                    log::debug!("Ingress VPN error: {}", e);
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
        })
    }
}

fn gsb_endpoint(node_id: &str, net_id: &str) -> GsbEndpoint {
    typed::service(format!("/net/{}/vpn/{}", node_id, net_id))
}
