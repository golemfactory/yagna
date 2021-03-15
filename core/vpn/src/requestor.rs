use crate::message::*;
use crate::network::VpnSupervisor;
use actix::prelude::*;
use actix_web::{web, HttpRequest, HttpResponse, Responder, ResponseError};
use actix_web_actors::ws;
use futures::channel::mpsc;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use ya_client_model::vpn::{AddNode, CreateNetwork};
use ya_client_model::ErrorMessage;
use ya_service_api_web::middleware::Identity;

pub const NET_API_PATH: &str = "/net-api/v1/";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

type Result<T> = std::result::Result<T, ApiError>;
type WsResult<T> = std::result::Result<T, ws::ProtocolError>;

pub fn web_scope() -> actix_web::Scope {
    let vpn = VpnSupervisor::default().start();
    actix_web::web::scope(NET_API_PATH)
        .data(vpn)
        .service(create_network)
        .service(remove_network)
        .service(add_node)
        .service(remove_node)
        .service(connect_tcp)
}

/// Creates a new private network
#[actix_web::post("/net")]
async fn create_network(
    vpn_sup: web::Data<Addr<VpnSupervisor>>,
    create_network: web::Json<CreateNetwork>,
    _identity: Identity,
) -> Result<impl Responder> {
    let create_network = create_network.into_inner();
    vpn_sup
        .send(VpnCreateNetwork::from(create_network))
        .await??;
    Ok(web::Json(()))
}

/// Removes an existing private network
#[actix_web::delete("/net/{net_id}")]
async fn remove_network(
    vpn_sup: web::Data<Addr<VpnSupervisor>>,
    path: web::Path<PathNetwork>,
    _identity: Identity,
) -> Result<impl Responder> {
    vpn_sup
        .send(VpnRemoveNetwork {
            net_id: path.into_inner().net_id,
        })
        .await??;
    Ok(web::Json(()))
}

/// Adds a new node to an existing private network
#[actix_web::post("/net/{net_id}/nodes")]
async fn add_node(
    vpn_sup: web::Data<Addr<VpnSupervisor>>,
    path: web::Path<PathNetwork>,
    add_node: web::Json<AddNode>,
    _identity: Identity,
) -> Result<impl Responder> {
    let add_node = add_node.into_inner();
    vpn_sup
        .send(VpnAddNode {
            net_id: path.into_inner().net_id,
            ip: add_node.ip,
            id: add_node.id,
        })
        .await??;
    Ok(web::Json(()))
}

/// Removes an existing node from a private network
#[actix_web::delete("/net/{net_id}/nodes/{node_id}")]
async fn remove_node(
    vpn_sup: web::Data<Addr<VpnSupervisor>>,
    path: web::Path<PathNetworkNode>,
    _identity: Identity,
) -> Result<impl Responder> {
    let model = path.into_inner();
    vpn_sup
        .send(VpnRemoveNode {
            net_id: model.net_id,
            id: model.node_id,
        })
        .await??;
    Ok(web::Json(()))
}

#[actix_web::get("/net/{net_id}/tcp/{ip}/{port}")]
async fn connect_tcp(
    vpn_sup: web::Data<Addr<VpnSupervisor>>,
    path: web::Path<PathConnect>,
    req: HttpRequest,
    stream: web::Payload,
    _identity: Identity,
) -> Result<HttpResponse> {
    let model = path.into_inner();
    let vpn = vpn_sup.send(VpnGetNetwork::new(model.net_id)).await??;

    let (ws_tx, ws_rx) = mpsc::channel(1);
    let vpn_rx = vpn
        .send(ConnectTcp {
            receiver: ws_rx,
            ip: model.ip,
            port: model.port,
        })
        .await??;

    Ok(ws::start(VpnWebSocket::new(ws_tx, vpn_rx), &req, stream)?)
}

pub struct VpnWebSocket {
    heartbeat: Instant,
    ws_tx: mpsc::Sender<Vec<u8>>,
    vpn_rx: Option<mpsc::Receiver<Vec<u8>>>,
}

impl VpnWebSocket {
    pub fn new(ws_tx: mpsc::Sender<Vec<u8>>, vpn_rx: mpsc::Receiver<Vec<u8>>) -> Self {
        VpnWebSocket {
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
                log::error!("VPN WS endpoint: VPN service has closed, shutting down");
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
                log::warn!("WebSocket network connection timed out");
                ctx.stop();
            } else {
                ctx.ping(b"");
            }
        });

        ctx.add_stream(self.vpn_rx.take().unwrap());
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::warn!("Network stopped");
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
        Ok(ctx.stop())
    }
}

#[derive(thiserror::Error, Debug)]
enum ApiError {
    #[error("VPN channel error: {0:?}")]
    ChannelError(#[from] actix::MailboxError),
    #[error("Web error: {0:?}")]
    WebError(#[from] actix_web::Error),
    #[error(transparent)]
    Vpn(#[from] ya_utils_networking::vpn::Error),
}

impl ResponseError for ApiError {
    fn error_response(&self) -> HttpResponse {
        match self {
            Self::ChannelError(_) | Self::Vpn(_) | Self::WebError(_) => {
                HttpResponse::BadRequest().json(ErrorMessage::new(self.to_string()))
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
