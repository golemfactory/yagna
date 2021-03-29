use crate::message::*;
use crate::network::VpnSupervisor;
use actix::prelude::*;
use actix_web::{web, HttpRequest, HttpResponse, Responder, ResponseError};
use actix_web_actors::ws;
use futures::channel::mpsc;
use futures::lock::Mutex;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ya_client_model::net::*;
use ya_client_model::ErrorMessage;
use ya_service_api_web::middleware::Identity;
use ya_utils_networking::vpn::Error as VpnError;

pub const NET_API_PATH: &str = "/net-api/v1/";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

type Result<T> = std::result::Result<T, ApiError>;
type WsResult<T> = std::result::Result<T, ws::ProtocolError>;

pub fn web_scope(vpn_sup: Arc<Mutex<VpnSupervisor>>) -> actix_web::Scope {
    actix_web::web::scope(NET_API_PATH)
        .data(vpn_sup)
        .service(get_networks)
        .service(create_network)
        .service(get_network)
        .service(remove_network)
        .service(get_addresses)
        .service(add_address)
        .service(get_nodes)
        .service(add_node)
        .service(remove_node)
        .service(get_connections)
        .service(connect_tcp)
}

/// Retrieves existing virtual private networks.
#[actix_web::get("/net")]
async fn get_networks(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    identity: Identity,
) -> impl Responder {
    let mut supervisor = vpn_sup.lock().await;
    let networks = supervisor.get_networks(&identity.identity);
    Ok::<_, ApiError>(web::Json(networks))
}

/// Creates a new virtual private network.
#[actix_web::post("/net")]
async fn create_network(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    model: web::Json<Network>,
    identity: Identity,
) -> impl Responder {
    let network = model.into_inner();
    let mut supervisor = vpn_sup.lock().await;
    supervisor
        .create_network(&identity.identity, network)
        .await?;
    Ok::<_, ApiError>(web::Json(()))
}

/// Retrieves an existing virtual private network.
#[actix_web::get("/net/{net_id}")]
async fn get_network(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let mut supervisor = vpn_sup.lock().await;
    let network = supervisor.get_network(&identity.identity, &path.net_id)?;
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
#[actix_web::get("/net/{net_id}/address")]
async fn get_addresses(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let fut = {
        let supervisor = vpn_sup.lock().await;
        supervisor.get_addresses(&identity.identity, &path.net_id)?
    };
    Ok::<_, ApiError>(web::Json(fut.await?))
}

/// Assigns a new address for the requestor within a virtual private network.
#[actix_web::post("/net/{net_id}/address")]
async fn add_address(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    model: web::Json<Address>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let address = model.into_inner();
    let fut = {
        let supervisor = vpn_sup.lock().await;
        supervisor.add_address(&identity.identity, &path.net_id, address.ip)?
    };
    Ok::<_, ApiError>(web::Json(fut.await?))
}

/// Retrieves requestor's addresses within a virtual private network.
#[actix_web::get("/net/{net_id}/node")]
async fn get_nodes(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let fut = {
        let supervisor = vpn_sup.lock().await;
        supervisor.get_nodes(&identity.identity, &path.net_id)?
    };
    Ok::<_, ApiError>(web::Json(fut.await?))
}

/// Adds a node to an existing virtual private network.
#[actix_web::post("/net/{net_id}/node")]
async fn add_node(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    model: web::Json<Node>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let node = model.into_inner();
    let fut = {
        let supervisor = vpn_sup.lock().await;
        supervisor.add_node(&identity.identity, &path.net_id, node.id, node.ip)?
    };
    Ok::<_, ApiError>(web::Json(fut.await?))
}

/// Removes an existing node from a virtual private network
#[actix_web::delete("/net/{net_id}/node/{node_id}")]
async fn remove_node(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetworkNode>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let fut = {
        let supervisor = vpn_sup.lock().await;
        supervisor.remove_node(&identity.identity, &path.net_id, path.node_id)?
    };
    Ok::<_, ApiError>(web::Json(fut.await?))
}

/// Retrieves existing connections (socket tuples) within a private network
#[actix_web::get("/net/{net_id}/tcp")]
async fn get_connections(
    vpn_sup: web::Data<Arc<Mutex<VpnSupervisor>>>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let fut = {
        let supervisor = vpn_sup.lock().await;
        supervisor.get_connections(&identity.identity, &path.net_id)?
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
    let path = path.into_inner();
    let fut = {
        let supervisor = vpn_sup.lock().await;
        supervisor.connect_tcp(&identity.identity, &path.net_id, &path.ip, path.port)?
    };

    let (ws_tx, vpn_rx) = fut.await.map_err(ApiError::from)?;
    Ok(ws::start(
        VpnWebSocket::new(path.net_id, ws_tx, vpn_rx),
        &req,
        stream,
    )?)
}

pub struct VpnWebSocket {
    network_id: String,
    heartbeat: Instant,
    ws_tx: mpsc::Sender<Vec<u8>>,
    vpn_rx: Option<mpsc::Receiver<Vec<u8>>>,
}

impl VpnWebSocket {
    pub fn new(
        network_id: String,
        ws_tx: mpsc::Sender<Vec<u8>>,
        vpn_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Self {
        VpnWebSocket {
            network_id,
            heartbeat: Instant::now(),
            ws_tx,
            vpn_rx: Some(vpn_rx),
        }
    }

    fn forward(&self, bytes: Vec<u8>, ctx: &mut <Self as Actor>::Context) {
        let addr = ctx.address();
        let mut tx = self.ws_tx.clone();

        async move {
            if let Err(_) = tx.send(bytes).await {
                let _ = addr.send(Shutdown {}).await;
            }
        }
        .into_actor(self)
        .wait(ctx);
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

        ctx.add_stream(self.vpn_rx.take().unwrap());
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::info!("VPN WebSocket: VPN {} connection stopped", self.network_id);
    }
}

impl StreamHandler<Vec<u8>> for VpnWebSocket {
    fn handle(&mut self, item: Vec<u8>, ctx: &mut Self::Context) {
        ctx.binary(item)
    }
}

impl StreamHandler<WsResult<ws::Message>> for VpnWebSocket {
    fn handle(&mut self, msg: WsResult<ws::Message>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(text)) => self.forward(text.into_bytes(), ctx),
            Ok(ws::Message::Binary(bytes)) => self.forward(bytes.to_vec(), ctx),
            Ok(ws::Message::Ping(msg)) => {
                self.heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.heartbeat = Instant::now();
            }
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
        Ok(ctx.stop())
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
                VpnError::NetNotFound(_) => HttpResponse::NotFound().json(ErrorMessage::new(&err)),
                VpnError::NetIdTaken(_) => HttpResponse::Conflict().json(ErrorMessage::new(&err)),
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
