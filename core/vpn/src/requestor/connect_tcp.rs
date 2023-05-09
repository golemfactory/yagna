use std::time::{Duration, Instant};

use actix::prelude::*;
use actix_web::{HttpRequest, HttpResponse, web};
use actix_web_actors::ws;
use futures::channel::mpsc;
use futures::prelude::*;
use serde::{Deserialize, Serialize};

use ya_service_api_web::middleware::Identity;
use ya_utils_networking::vpn::Protocol;
use ya_utils_networking::vpn::stack::connection::ConnectionMeta;

use crate::message::*;
use crate::network::{VpnSupervisorRef};

use super::Result;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

type WsResult<T> = std::result::Result<T, ws::ProtocolError>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct PathConnect {
    net_id: String,
    ip: String,
    port: u16,
}

/// Initiates a new TCP connection via WebSockets to the destination address.
#[actix_web::get("/net/{net_id}/tcp/{ip}/{port}")]
pub(super) async fn connect_tcp(
    vpn_sup: web::Data<VpnSupervisorRef>,
    path: web::Path<PathConnect>,
    req: HttpRequest,
    stream: web::Payload,
    identity: Identity,
) -> Result<HttpResponse> {
    let path = path.into_inner();
    let vpn = {
        let supervisor = vpn_sup.read().await;
        supervisor.get_network(&identity.identity, &path.net_id)?
    };
    let conn = vpn
        .send(Connect {
            protocol: Protocol::Tcp,
            address: path.ip.to_string(),
            port: path.port,
        })
        .await??;
    Ok(ws::start(
        VpnWebSocket::new(path.net_id, conn),
        &req,
        stream,
    )?)
}

pub struct VpnWebSocket {
    network_id: String,
    heartbeat: Instant,
    vpn: Recipient<Packet>,
    vpn_rx: Option<mpsc::Receiver<Vec<u8>>>,
    meta: ConnectionMeta,
}

impl VpnWebSocket {
    pub fn new(network_id: String, conn: UserConnection) -> Self {
        VpnWebSocket {
            network_id,
            heartbeat: Instant::now(),
            vpn: conn.vpn,
            vpn_rx: Some(conn.rx),
            meta: conn.stack_connection.meta,
        }
    }

    fn forward(&self, data: Vec<u8>, ctx: &mut <Self as Actor>::Context) {
        // packet tracing is also done when the packet data is no longer available,
        // so we have to make a temporary copy. This incurs no runtime overhead on builds
        // without the feature packet-trace-enable.
        #[cfg(feature = "packet-trace-enable")]
            let data_trace = data.clone();

        ya_packet_trace::packet_trace!("VpnWebSocket::Tx::1", { &data_trace });

        let vpn = self.vpn.clone();
        vpn.send(Packet {
            data,
            meta: self.meta,
        })
            .into_actor(self)
            .map(move |result, this, ctx| {
                if result.is_err() {
                    log::error!("VPN WebSocket: VPN {} no longer exists", this.network_id);
                    let _ = ctx.address().do_send(Shutdown {});
                }
            })
            .wait(ctx);

        ya_packet_trace::packet_trace!("VpnWebSocket::Tx::2", { &data_trace });
    }
}

impl Actor for VpnWebSocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > CLIENT_TIMEOUT {
                log::warn!("VPN WebSocket: VPN {} connection timed out", act.network_id);
                ctx.stop();
            } else {
                ctx.ping(b"");
            }
        });

        ctx.add_stream(self.vpn_rx.take().unwrap().map(|packet| {
            ya_packet_trace::packet_trace!("VpnWebSocket::Rx", { &packet });
            packet
        }));
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::info!("VPN WebSocket: VPN {} connection stopped", self.network_id);
    }
}

impl StreamHandler<Vec<u8>> for VpnWebSocket {
    fn handle(&mut self, data: Vec<u8>, ctx: &mut Self::Context) {
        ctx.binary(data)
    }
}

impl StreamHandler<WsResult<ws::Message>> for VpnWebSocket {
    fn handle(&mut self, msg: WsResult<ws::Message>, ctx: &mut Self::Context) {
        self.heartbeat = Instant::now();
        match msg {
            Ok(ws::Message::Text(text)) => self.forward(text.into_bytes().to_vec(), ctx),
            Ok(ws::Message::Binary(bytes)) => self.forward(bytes.to_vec(), ctx),
            Ok(ws::Message::Ping(msg)) => {
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {}
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            }
            _ => {
                ctx.stop();
            }
        }
    }
}

impl Handler<Shutdown> for VpnWebSocket {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        log::warn!("VPN WebSocket: VPN {} is shutting down", self.network_id);
        ctx.stop();
        Ok(())
    }
}
