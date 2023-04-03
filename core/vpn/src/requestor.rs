#![allow(clippy::let_unit_value)]

use crate::message::*;
use crate::network::{Vpn, VpnSupervisor};
use actix::prelude::*;
use actix_web::{web, HttpRequest, HttpResponse, Responder, ResponseError};
use actix_web_actors::ws;
use actix_web_actors::ws::WsResponseBuilder;
use futures::channel::mpsc;
use futures::lock::Mutex;
use futures::FutureExt;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ya_client_model::net::*;
use ya_client_model::{ErrorMessage, NodeId};
use ya_service_api_web::middleware::Identity;
use ya_utils_networking::vpn::stack::connection::ConnectionMeta;
use ya_utils_networking::vpn::{Error as VpnError, Protocol};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

type Result<T> = std::result::Result<T, ApiError>;
type WsResult<T> = std::result::Result<T, ws::ProtocolError>;

const API_ROOT_PATH: &str = "/net-api";

pub fn web_scope(vpn_sup: Arc<Mutex<VpnSupervisor>>) -> actix_web::Scope {
    let api_v1_subpath = api_subpath(NET_API_V1_VPN_PATH);
    let api_v2_subpath = api_subpath(NET_API_V2_VPN_PATH);

    web::scope(API_ROOT_PATH)
        .app_data(web::Data::new(vpn_sup))
        .service(vpn_web_scope(api_v1_subpath))
        .service(vpn_web_scope(api_v2_subpath))
}

fn api_subpath(path: &str) -> &str {
    path.trim_start_matches(API_ROOT_PATH)
}

fn vpn_web_scope(path: &str) -> actix_web::Scope {
    web::scope(path)
        .service(get_networks)
        .service(create_network)
        .service(get_network)
        .service(remove_network)
        .service(get_addresses)
        .service(add_address)
        .service(get_nodes)
        .service(add_node)
        .service(remove_node)
        .service(connect_tcp)
        .service(connect_raw)
}

/// Retrieves existing virtual private networks.
#[actix_web::get("/net")]
async fn get_networks(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    identity: Identity,
) -> impl Responder {
    let networks = {
        let supervisor = vpn_sup.lock().await;
        supervisor.get_networks(&identity.identity)
    };
    Ok::<_, ApiError>(web::Json(networks))
}

/// Creates a new virtual private network.
#[actix_web::post("/net")]
async fn create_network(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    model: web::Json<NewNetwork>,
    identity: Identity,
) -> impl Responder {
    let network = model.into_inner();
    let mut supervisor = vpn_sup.lock().await;
    let network = supervisor
        .create_network(identity.identity, network)
        .await?;
    Ok::<_, ApiError>(web::Json(network))
}

/// Retrieves an existing virtual private network.
#[actix_web::get("/net/{net_id}")]
async fn get_network(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let network = {
        let supervisor = vpn_sup.lock().await;
        supervisor.get_blueprint(&identity.identity, &path.net_id)?
    };
    Ok::<_, ApiError>(web::Json(network))
}

/// Removes an existing virtual private network.
#[actix_web::delete("/net/{net_id}")]
async fn remove_network(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let fut = {
        let mut supervisor = vpn_sup.lock().await;
        supervisor.remove_network(&identity.identity, &path.net_id)?
    };
    Ok::<_, ApiError>(web::Json(fut.await?))
}

/// Retrieves requestor's addresses within a virtual private network.
#[actix_web::get("/net/{net_id}/addresses")]
async fn get_addresses(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let vpn = {
        let supervisor = vpn_sup.lock().await;
        supervisor.get_network(&identity.identity, &path.net_id)?
    };
    let response = vpn.send(GetAddresses {}).await??;
    Ok::<_, ApiError>(web::Json(response))
}

/// Assigns a new address for the requestor within a virtual private network.
#[actix_web::post("/net/{net_id}/addresses")]
async fn add_address(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    model: web::Json<Address>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let vpn = {
        let supervisor = vpn_sup.lock().await;
        supervisor.get_network(&identity.identity, &path.net_id)?
    };
    let address = model.into_inner().ip;
    let response = vpn.send(AddAddress { address }).await??;
    Ok::<_, ApiError>(web::Json(response))
}

/// Retrieves requestor's addresses within a virtual private network.
#[actix_web::get("/net/{net_id}/nodes")]
async fn get_nodes(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let vpn = {
        let supervisor = vpn_sup.lock().await;
        supervisor.get_network(&identity.identity, &path.net_id)?
    };
    let response = vpn.send(GetNodes {}).await??;
    Ok::<_, ApiError>(web::Json(response))
}

/// Adds a node to an existing virtual private network.
#[actix_web::post("/net/{net_id}/nodes")]
async fn add_node(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    model: web::Json<Node>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let vpn = {
        let supervisor = vpn_sup.lock().await;
        supervisor.get_network(&identity.identity, &path.net_id)?
    };
    let node = model.into_inner();
    let response = vpn
        .send(AddNode {
            id: node.id,
            address: node.ip,
        })
        .await??;
    Ok::<_, ApiError>(web::Json(response))
}

/// Removes an existing node from a virtual private network
#[actix_web::delete("/net/{net_id}/nodes/{node_id}")]
async fn remove_node(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetworkNode>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let fut = {
        let mut supervisor = vpn_sup.lock().await;
        supervisor.remove_node(&identity.identity, &path.net_id, path.node_id)?
    };
    Ok::<_, ApiError>(web::Json(fut.await?))
}

/// Initiates a new TCP connection via WebSockets to the destination address.
#[actix_web::get("/net/{net_id}/tcp/{ip}/{port}")]
async fn connect_tcp(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathConnect>,
    req: HttpRequest,
    stream: web::Payload,
    identity: Identity,
) -> Result<HttpResponse> {
    log::warn!("connect_tcp called {:?}", path);
    let path = path.into_inner();
    let vpn = {
        let supervisor = vpn_sup.lock().await;
        supervisor.get_network(&identity.identity, &path.net_id)?
    };
    let conn = vpn
        .send(ConnectTcp {
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
    pub fn new(network_id: String, conn: UserTcpConnection) -> Self {
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

/// Initiates a new RAW connection via WebSockets to the destination address.
#[actix_web::get("/net/{net_id}/raw/from/{src}/to/{dst}")]
async fn connect_raw(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<(String, IpAddr, IpAddr)>,
    req: HttpRequest,
    stream: web::Payload,
    identity: Identity,
) -> Result<HttpResponse> {
    let (net_id, src, dst_ip) = path.into_inner();
    log::info!("vpn {net_id} connection from {src} to {dst_ip}");
    let vpn = {
        let supervisor = vpn_sup.lock().await;
        supervisor.get_network(&identity.identity, &net_id)
    }?;

    let nodes = vpn.send(GetNodes).await??;
    let dst_ip_str = dst_ip.to_string();
    let dst_node = match nodes.into_iter().find(|n| n.ip == dst_ip_str) {
        Some(n) => n,
        None => {
            return Err(ApiError::Vpn(VpnError::ConnectionError(
                "destination address not found".to_string(),
            )))
        }
    };

    let raw_socket_desc = RawSocketDesc {
        dst_addr: dst_ip,
        src_addr: src,
        dst_id: dst_node.id.clone(),
    };

    let conn = vpn
        .send(ConnectRaw {
            raw_socket_desc: raw_socket_desc.clone(),
        })
        .await??;

    let (_addr, response) = WsResponseBuilder::new(
        VpnRawSocket {
            node_id: identity.identity.to_string(),
            dst_node,
            network_id: net_id,
            raw_conn_desc: raw_socket_desc,
            heartbeat: Instant::now(),
            vpn_rx: Some(conn.rx),
            vpn_service: vpn,
        },
        &req,
        stream,
    )
    .start_with_addr()?;

    Ok(response)
}

pub struct VpnRawSocket {
    node_id: String,
    dst_node: Node,
    network_id: String,
    raw_conn_desc: RawSocketDesc,
    heartbeat: Instant,
    vpn_rx: Option<mpsc::Receiver<Vec<u8>>>,
    vpn_service: Addr<Vpn>,
}

impl VpnRawSocket {
    fn forward(&self, data: Vec<u8>, ctx: &mut <Self as Actor>::Context) {
        use ya_net::*;

        let dst_node_id: NodeId = self.dst_node.id.parse().unwrap();
        let current_node_id = self.node_id.clone();

        #[cfg(feature = "trace-raw-packets")]
        use std::sync::atomic::{AtomicU64, Ordering};
        #[cfg(feature = "trace-raw-packets")]
        static PACKET_NO: AtomicU64 = AtomicU64::new(0);
        #[cfg(feature = "trace-raw-packets")]
        let packet_no = PACKET_NO.fetch_add(1, Ordering::Relaxed);

        #[cfg(feature = "trace-raw-packets")]
        log::info!(
            "VPN WebSocket: VPN {} forwarding packet to {}",
            packet_no,
            dst_node_id
        );
        let vpn_node = dst_node_id.service_udp(&format!("/public/vpn/{}/raw", self.network_id));

        ctx.wait(
            async move {
                let _res = match vpn_node.push_raw_as(&current_node_id, data).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        log::error!("failed to send packet {:?}", e);
                        Err(anyhow::anyhow!("failed to send packet {:?}", e))
                    }
                };
                #[cfg(feature = "trace-raw-packets")]
                log::info!("Pushed message to {}", packet_no);

                Ok::<_, anyhow::Error>(())
            }
            .then(|v| match v {
                Err(e) => fut::ready(log::error!("failed to send packet {:?}", e)),
                Ok(()) => fut::ready(()),
            })
            .into_actor(self),
        );
    }
}

impl Actor for VpnRawSocket {
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

        ctx.add_stream(self.vpn_rx.take().unwrap());
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::info!("VPN RawSocket: VPN {} connection stopped", self.network_id);
        let _ = self.vpn_service.send(DisconnectRaw {
            raw_socket_desc: self.raw_conn_desc.clone(),
            reason: DisconnectReason::SocketClosed,
        });
    }
}

impl StreamHandler<Vec<u8>> for VpnRawSocket {
    fn handle(&mut self, data: Vec<u8>, ctx: &mut Self::Context) {
        ctx.binary(data)
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        log::info!("VPN RawSocket: UserRawConnection stream closed");
        ctx.stop();
    }
}

impl StreamHandler<WsResult<ws::Message>> for VpnRawSocket {
    fn handle(&mut self, msg: WsResult<ws::Message>, ctx: &mut Self::Context) {
        self.heartbeat = Instant::now();
        match msg {
            Ok(ws::Message::Text(_)) => {
                log::warn!("VPN RawSocket: Received text message, not supported");
            }
            Ok(ws::Message::Binary(bytes)) => self.forward(bytes.to_vec(), ctx),
            Ok(ws::Message::Ping(msg)) => {
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {}
            Ok(ws::Message::Close(reason)) => {
                log::warn!("Received message close, close reason: {:?}", reason);
                ctx.close(reason);
                ctx.stop();
            }
            Ok(ws::Message::Continuation(_)) => {
                log::warn!(
                    "VPN RawSocket: VPN {} connection error: continuation not supported",
                    self.network_id
                );
            }
            Ok(ws::Message::Nop) => {
                log::warn!(
                    "VPN RawSocket: VPN {} connection error: nop not supported",
                    self.network_id
                );
            }
            Err(e) => {
                log::error!(
                    "VPN RawSocket: VPN {} connection error: {:?}",
                    self.network_id,
                    e
                );
                ctx.stop();
            }
        }
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        log::info!("VPN RawSocket: Websocket stream closed");
        ctx.stop();
    }
}

impl Handler<Shutdown> for VpnRawSocket {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        log::info!("VPN RawSocket: VPN {} is shutting down", self.network_id);
        ctx.stop();
        Ok(())
    }
}

#[derive(thiserror::Error, Debug)]
enum ApiError {
    #[error("VPN communication error: {0:?}")]
    ChannelError(#[from] actix::MailboxError),
    #[error("Request error: {0:?}")]
    WebError(#[from] actix_web::Error),
    #[error(transparent)]
    Vpn(#[from] VpnError),
}

impl ResponseError for ApiError {
    fn error_response(&self) -> HttpResponse {
        match self {
            Self::Vpn(err) => match err {
                VpnError::IpAddrTaken(_) => HttpResponse::Conflict().json(ErrorMessage::new(&err)),
                VpnError::NetIdTaken(_) => HttpResponse::Conflict().json(ErrorMessage::new(&err)),
                VpnError::NetNotFound => HttpResponse::NotFound().json(ErrorMessage::new(&err)),
                VpnError::ConnectionTimeout => HttpResponse::GatewayTimeout().finish(),
                VpnError::Forbidden => HttpResponse::Forbidden().finish(),
                VpnError::Cancelled => {
                    HttpResponse::InternalServerError().json(ErrorMessage::new(&err))
                }
                _ => HttpResponse::BadRequest().json(ErrorMessage::new(&err)),
            },
            Self::ChannelError(_) | Self::WebError(_) => {
                HttpResponse::BadRequest().json(ErrorMessage::new(&self))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PathNetwork {
    net_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PathNetworkNode {
    net_id: String,
    node_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PathConnect {
    net_id: String,
    ip: String,
    port: u16,
}

#[test]
fn test_to_detect_breaking_ya_client_const_changes() {
    assert!(
        api_subpath(NET_API_V1_VPN_PATH).len() < NET_API_V1_VPN_PATH.len(),
        "ya-client const NET_API_V1_VPN_PATH changed"
    );
    assert!(
        api_subpath(NET_API_V2_VPN_PATH).len() < NET_API_V2_VPN_PATH.len(),
        "ya-client const NET_API_V2_VPN_PATH changed"
    )
}
