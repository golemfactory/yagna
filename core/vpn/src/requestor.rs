#![allow(clippy::let_unit_value)]

use crate::message::*;
use crate::network::VpnSupervisor;
use actix::prelude::*;
use actix_web::{web, HttpRequest, HttpResponse, Responder, ResponseError};
use actix_web_actors::ws;
use anyhow::bail;
use futures::channel::mpsc;
use futures::lock::Mutex;
use futures::FutureExt;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ya_client_model::net::*;
use ya_client_model::{ErrorMessage, NodeId};
use ya_core_model::activity::{VpnPacket};
use ya_core_model::net::RemoteEndpoint;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::RpcEndpoint;
use ya_utils_networking::vpn::stack::connection::ConnectionMeta;
use ya_utils_networking::vpn::{
    Error as VpnError, IpPacket, IpV4Field, PeekPacket, Protocol, UdpField, UdpPacket,
};

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
            packet_type: PacketType::Tcp,
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

// only for echo server
fn reverse_udp(frame: &Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let ip_packet = match IpPacket::peek(frame) {
        Ok(_) => IpPacket::packet(frame),
        _ => bail!("Error peeking IP packet"),
    };

    if ip_packet.protocol() != Protocol::Udp as u8 {
        return Ok(frame.to_vec());
    }

    let src = ip_packet.src_address();
    let dst = ip_packet.dst_address();

    println!("Src: {:?}, Dst: {:?}", src, dst);

    let udp_data = ip_packet.payload();
    let _udp_data_len = udp_data.len();

    let udp_packet = match UdpPacket::peek(udp_data) {
        Ok(_) => UdpPacket::packet(udp_data),
        _ => bail!("Error peeking UDP packet"),
    };

    let src_port = udp_packet.src_port();
    let dst_port = udp_packet.dst_port();
    println!("Src port: {:?}, Dst port: {:?}", src_port, dst_port);

    let content = &udp_data[UdpField::PAYLOAD];

    match std::str::from_utf8(content) {
        Ok(content_str) => println!("Content (string): {content_str:?}"),
        Err(_e) => println!("Content (binary): {:?}", content),
    };

    let mut reversed = frame.clone();
    reversed[IpV4Field::SRC_ADDR].copy_from_slice(&dst);
    reversed[IpV4Field::DST_ADDR].copy_from_slice(&src);

    let reversed_udp_data = &mut reversed[ip_packet.payload_off()..];

    reversed_udp_data[UdpField::SRC_PORT].copy_from_slice(&udp_data[UdpField::DST_PORT]);
    reversed_udp_data[UdpField::DST_PORT].copy_from_slice(&udp_data[UdpField::SRC_PORT]);

    Ok(reversed)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ConnectRawArgs {
    net_id: String,
    requestor_ip: String,
    dst_ip: String,
}


/// Initiates a new RAW connection via WebSockets to the destination address.
// #[actix_web::get("/net/{net_id}/raw/from/{port}/to/{ip}")]
#[actix_web::get("/net/{net_id}/raw/{ip}/{port}")]
async fn connect_raw(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathConnect>,
    req: HttpRequest,
    stream: web::Payload,
    identity: Identity,
) -> Result<HttpResponse> {
    log::warn!("Connect raw called {:?}", path);

    let net_id = path.net_id.clone();
    /*let requestor_ip = IpAddr::from_str(&path.requestor_ip).map_err(|e| {
        ApiError::Vpn(VpnError::ConnectionError(format!("invalid requestor IP: {}", e)))
    })?;*/
    let dst_ip = IpAddr::from_str(&path.ip).map_err(|e| {
        ApiError::Vpn(VpnError::ConnectionError(format!("invalid destination IP: {}", e)))
    })?;

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

    let conn = vpn
        .send(ConnectRaw {
            address: dst_ip_str,
        })
        .await??;

    Ok(ws::start(
        VpnRawSocket::new(net_id, dst_ip, dst_ip, dst_node),
        &req,
        stream,
    )?)
}

pub struct VpnRawSocket {
    network_id: String,
    src_ip: IpAddr,
    dst_ip: IpAddr,
    dst_node: Node,
    heartbeat: Instant,
}

impl VpnRawSocket {
    pub fn new(network_id: String, src_ip: IpAddr, dst_ip: IpAddr, dst_node: Node) -> Self {
        VpnRawSocket {
            network_id,
            src_ip,
            dst_ip,
            dst_node,
            heartbeat: Instant::now(),
        }
    }

    fn forward(&self, data: Vec<u8>, ctx: &mut <Self as Actor>::Context) {
        use ya_net::*;

        let dst_node_id: NodeId = self.dst_node.id.parse().unwrap();
        let vpn_node = dst_node_id.service(&format!("/public/vpn/{}", self.network_id));

        ctx.spawn(
            async move {
                let res = vpn_node.send(VpnPacket(data)).await??;

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
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::warn!("VPN WebSocket: VPN {} connection stopped", self.network_id);
    }
}

/*impl StreamHandler<Vec<u8>> for VpnRawSocket {
    fn handle(&mut self, data: Vec<u8>, ctx: &mut Self::Context) {
        ctx.binary(data)
    }
}*/

impl StreamHandler<WsResult<ws::Message>> for VpnRawSocket {
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

impl Handler<Shutdown> for VpnRawSocket {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        log::warn!("VPN WebSocket: VPN {} is shutting down", self.network_id);
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
