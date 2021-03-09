use crate::error::Error;
use crate::message::Shutdown;
use crate::state::Deployment;
use crate::Result;
use actix::prelude::*;
use futures::channel::mpsc;
use futures::future;
use futures::{FutureExt, SinkExt, Stream, StreamExt, TryFutureExt, TryStreamExt};
use std::convert::TryFrom;
use std::ops::Not;
use std::path::Path;
use tokio::io;

use ya_core_model::activity;
use ya_core_model::activity::VpnControl;
use ya_runtime_api::server::{CreateNetwork, Network, NetworkEndpoint, RuntimeService};
use ya_service_bus::{actix_rpc, typed, typed::Endpoint as GsbEndpoint, RpcEnvelope};
use ya_utils_networking::vpn::error::Error as VpnError;
use ya_utils_networking::vpn::{
    ArpField, ArpPacket, EtherFrame, EtherType, IpPacket, State as VpnState, MAX_FRAME_SIZE,
};

pub(crate) async fn start_vpn<R: RuntimeService>(
    service: &R,
    activity_id: Option<String>,
    deployment: Deployment,
) -> Result<Option<Addr<Vpn>>> {
    if activity_id.is_none() || !deployment.networking() {
        return Ok(None);
    }

    let hosts = deployment.hosts.clone();
    let networks = deployment
        .networks
        .values()
        .map(|net| {
            let ip = net.addr();
            let gateway = net
                .hosts()
                .find(|ip_| ip_ != &ip)
                .ok_or_else(|| VpnError::NetAddrTaken(ip))?;
            Ok(Network {
                ipv6: ip.is_ipv6(),
                addr: ip.to_string(),
                gateway: gateway.to_string(),
                mask: net.netmask().to_string(),
            })
        })
        .collect::<Result<_>>()?;

    let response = service
        .create_network(CreateNetwork { networks, hosts })
        .await
        .map_err(|e| Error::Other(format!("Network setup error: {:?}", e)))?;
    let endpoint = match response.endpoint {
        Some(endpoint) => VpnEndpoint::connect(endpoint).await?,
        None => return Err(Error::Other("VPN endpoint already connected".into()).into()),
    };

    let vpn = Vpn::try_new(activity_id.unwrap(), endpoint, deployment)?;
    Ok(Some(vpn.start()))
}

pub(crate) struct Vpn {
    activity_id: String,
    state: VpnState<GsbEndpoint>,
    endpoint: VpnEndpoint,
}

impl Vpn {
    fn try_new(activity_id: String, endpoint: VpnEndpoint, deployment: Deployment) -> Result<Self> {
        let mut state = VpnState::default();
        state.create(deployment.networks)?;
        state.join(deployment.nodes, gsb_endpoint)?;

        Ok(Self {
            activity_id,
            state,
            endpoint,
        })
    }

    fn handle_ip(&mut self, frame: EtherFrame, ctx: &mut Context<Self>) {
        let ip_pkt = IpPacket::packet(frame.payload());
        log::trace!("Egress packet to {:?}", ip_pkt.dst_address());

        if ip_pkt.is_broadcast() {
            let pkt: Vec<_> = frame.into();
            let futs = self
                .state
                .endpoints()
                .values()
                .map(|e| e.call(activity::VpnPacket(pkt.clone())))
                .collect::<Vec<_>>();
            futs.is_empty().not().then(|| {
                let fut = future::join_all(futs).then(|_| future::ready(()));
                ctx.spawn(fut.into_actor(self))
            });
        } else {
            let ip = ip_pkt.dst_address();
            let endpoint = self.state.endpoints().get(ip).cloned();
            match endpoint {
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
        match self.state.endpoints().get(ip).cloned() {
            Some(endpoint) => self.forward_frame(endpoint, frame, ctx),
            None => log::debug!("No endpoint for {:?}", ip),
        }
    }

    fn forward_frame(&mut self, endpoint: GsbEndpoint, frame: EtherFrame, ctx: &mut Context<Self>) {
        let pkt: Vec<_> = frame.into();
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
        let srv_id = activity::exeunit::bus_id(&self.activity_id);
        actix_rpc::bind::<activity::VpnControl>(&srv_id, ctx.address().recipient());

        self.state.networks().keys().for_each(|net| {
            let vpn_id = activity::exeunit::network_id(net);
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

        let srv_id = activity::exeunit::bus_id(&self.activity_id);
        let networks = self.state.networks().clone();

        ctx.wait(
            async move {
                let _ = typed::unbind(&srv_id).await;
                for net in networks.keys() {
                    let vpn_id = activity::exeunit::network_id(net);
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
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(err) => return log::debug!("VPN error (egress): {}", err),
        };
        let frame = match EtherFrame::try_from(bytes) {
            Ok(frame) => frame,
            Err(err) => return log::debug!("VPN frame error (egress): {}", err),
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
        log::trace!("Ingress packet");

        let mut tx = self.endpoint.tx.clone();
        ctx.spawn(
            async move {
                if let Err(e) = tx.send(Ok(packet.into_inner().0)).await {
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
        match msg.into_inner() {
            VpnControl::RemoveNodes(ids) => self.state.leave(ids),
            VpnControl::AddNodes(nodes) => {
                let mut deployment = Deployment::default();
                deployment.extend_nodes(nodes)?;
                self.state
                    .join(deployment.nodes, gsb_endpoint)
                    .map_err(Error::from)?;
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

fn gsb_endpoint(node_id: &str, net_id: &str) -> GsbEndpoint {
    typed::service(format!("/net/{}/vpn/{}", node_id, net_id))
}
