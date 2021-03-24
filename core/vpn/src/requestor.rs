use crate::message::*;
use crate::network::VpnSupervisor;
use actix::prelude::*;
use actix_web::{web, HttpRequest, HttpResponse, Responder, ResponseError};
use actix_web_actors::ws;
use futures::channel::mpsc;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use ya_client_model::net::*;
use ya_client_model::ErrorMessage;
use ya_service_api_web::middleware::Identity;

pub const NET_API_PATH: &str = "/net-api/v1/";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(60);

type Result<T> = std::result::Result<T, ApiError>;
type WsResult<T> = std::result::Result<T, ws::ProtocolError>;

lazy_static::lazy_static! {
    static ref VPN_SUPERVISOR: Addr<VpnSupervisor> = VpnSupervisor::from_registry();
}

pub fn web_scope() -> actix_web::Scope {
    actix_web::web::scope(NET_API_PATH)
        .service(create_network)
        .service(remove_network)
        .service(add_node)
        .service(remove_node)
        .service(connect_tcp)
}

/// Creates a new private network
#[actix_web::post("/net")]
async fn create_network(
    create_network: web::Json<CreateNetwork>,
    identity: Identity,
) -> Result<impl Responder> {
    let requestor_id = identity.identity.to_string();
    let create_network = create_network.into_inner();
    (*VPN_SUPERVISOR)
        .send(VpnCreateNetwork::new(requestor_id, create_network))
        .await??;
    Ok(web::Json(()))
}

/// Removes an existing private network
#[actix_web::delete("/net/{net_id}")]
async fn remove_network(
    path: web::Path<PathNetwork>,
    _identity: Identity,
) -> Result<impl Responder> {
    (*VPN_SUPERVISOR)
        .send(VpnRemoveNetwork {
            net_id: path.into_inner().net_id,
        })
        .await??;
    Ok(web::Json(()))
}

/// Adds a new node to an existing private network
#[actix_web::post("/net/{net_id}/nodes")]
async fn add_node(
    path: web::Path<PathNetwork>,
    add_node: web::Json<Node>,
    _identity: Identity,
) -> Result<impl Responder> {
    let add_node = add_node.into_inner();
    (*VPN_SUPERVISOR)
        .send(VpnAddNode {
            net_id: path.into_inner().net_id,
            address: add_node.address,
            id: add_node.id,
        })
        .await??;
    Ok(web::Json(()))
}

/// Removes an existing node from a private network
#[actix_web::delete("/net/{net_id}/nodes/{node_id}")]
async fn remove_node(
    path: web::Path<PathNetworkNode>,
    _identity: Identity,
) -> Result<impl Responder> {
    let model = path.into_inner();
    (*VPN_SUPERVISOR)
        .send(VpnRemoveNode {
            net_id: model.net_id,
            id: model.node_id,
        })
        .await??;
    Ok(web::Json(()))
}

#[actix_web::get("/net/{net_id}/tcp/{ip}/{port}")]
async fn connect_tcp(
    path: web::Path<PathConnect>,
    req: HttpRequest,
    stream: web::Payload,
    _identity: Identity,
) -> Result<HttpResponse> {
    let model = path.into_inner();
    let vpn = (*VPN_SUPERVISOR)
        .send(VpnGetNetwork::new(model.net_id.clone()))
        .await??;

    let (ws_tx, ws_rx) = mpsc::channel(1);
    let vpn_rx = vpn
        .send(ConnectTcp {
            receiver: ws_rx,
            address: model.ip,
            port: model.port,
        })
        .await??;

    Ok(ws::start(
        VpnWebSocket::new(model.net_id, ws_tx, vpn_rx),
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
                log::warn!("VPN WebSocket: connection timed out");
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
