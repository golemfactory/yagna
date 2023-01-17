mod api;
mod services;

use actix::prelude::*;
use actix::{dev::MessageResponse, Actor, Addr, Handler, MailboxError, StreamHandler};
use actix_http::{
    ws::{CloseReason, ProtocolError},
    StatusCode,
};
use actix_web::ResponseError;
use actix_web_actors::ws;

use flexbuffers::Reader;

use serde::{Deserialize, Serialize};
use serde_json::json;
use services::AService;

use thiserror::Error;
use ya_service_api_interfaces::Provider;

pub const GSB_API_PATH: &str = "/gsb-api/v1";

pub struct GsbApiService;

impl GsbApiService {
    pub async fn gsb<Context>(_: Context) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn rest<Context: Provider<Self, ()>>(_ctx: &Context) -> actix_web::Scope {
        api::web_scope()
    }
}

#[derive(Error, Debug)]
enum GsbApiError {
    //TODO add msg
    #[error("Bad request")]
    BadRequest,
    //TODO add msg
    #[error("Internal error")]
    InternalError(String),
    #[error(transparent)]
    Any(#[from] anyhow::Error),
}

impl From<ya_service_bus::Error> for GsbApiError {
    fn from(value: ya_service_bus::Error) -> Self {
        GsbApiError::InternalError(format!("GSB error: {value}"))
    }
}

impl From<MailboxError> for GsbApiError {
    fn from(value: MailboxError) -> Self {
        GsbApiError::InternalError(format!("Actix error: {value}"))
    }
}

impl From<serde_json::Error> for GsbApiError {
    fn from(value: serde_json::Error) -> Self {
        GsbApiError::InternalError(format!("Serde error {value}"))
    }
}

impl From<actix_web::Error> for GsbApiError {
    fn from(value: actix_web::Error) -> Self {
        GsbApiError::InternalError(format!("Actix error: {value}"))
    }
}

#[derive(Error, Debug)]
enum WsApiError {
    #[error("Internal Error")]
    InternalError,
    #[error(transparent)]
    Any(#[from] anyhow::Error),
}

impl ResponseError for GsbApiError {
    fn status_code(&self) -> StatusCode {
        match *self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Message, Serialize, Deserialize, Debug)]
#[rtype(result = "Result<(), anyhow::Error>")]
struct WsRequest {
    id: String,
    component: String,
    msg: Vec<u8>,
}

#[derive(Message, Debug)]
#[rtype(result = "Result<(), anyhow::Error>")]
pub(crate) struct WsResponse {
    pub id: String,
    pub response: WsResponseMsg,
}

#[derive(Debug)]
pub(crate) enum WsResponseMsg {
    Message(Vec<u8>),
    Error(GsbApiError),
}

pub(crate) struct WsMessagesHandler {
    // pub responders: Arc<RwLock<HashMap<String, Sender<WsResult>>>>,
    service: Addr<AService>,
}

impl WsMessagesHandler {
    async fn handle(service: Addr<AService>, msg: bytes::Bytes) {
        match Reader::get_root(&*msg) {
            Ok(buffer) => {
                let response = buffer.as_map();
                //TODO handle errors
                let id = response.index("id").unwrap().as_str().to_string();
                let msg = response.index("payload").unwrap().as_blob().0.to_vec();
                let response = WsResponse {
                    id,
                    response: WsResponseMsg::Message(msg),
                };
                log::info!("WsResponse: {}", response.id);
                match service.send(response).await {
                    Ok(res) => {
                        if let Err(err) = res {
                            log::error!("Failed to handle WS msg: {err}");
                            //TODO error response?
                        }
                    }
                    Err(err) => {
                        log::error!("Internal error: {err}");
                        //TODO error response?
                    }
                }
            }
            Err(err) => {
                //TODO shutdown service connections?
                log::error!("WS response error: {err}");
            }
        }
    }
}

impl Actor for WsMessagesHandler {
    type Context = ws::WebsocketContext<Self>;
}

impl Handler<WsRequest> for WsMessagesHandler {
    type Result = <WsRequest as Message>::Result;

    fn handle(&mut self, request: WsRequest, ctx: &mut Self::Context) -> Self::Result {
        log::info!("WS request (id: {}, component: {})", request.id, request.component);
        let msg = flexbuffers::to_vec(&request)
            .map_err(|err| anyhow::anyhow!("Failed to serialize msg: {}", err))?;
        ctx.binary(msg);
        Ok(())
    }
}

impl StreamHandler<Result<actix_http::ws::Message, ProtocolError>> for WsMessagesHandler {
    fn handle(
        &mut self,
        item: Result<actix_http::ws::Message, ProtocolError>,
        ctx: &mut Self::Context,
    ) {
        match item {
            Ok(msg) => match msg {
                ws::Message::Binary(msg) => {
                    log::info!("Binary (len {})", msg.len());
                    let service = self.service.clone();
                    ctx.spawn(actix::fut::wrap_future(Self::handle(service, msg)));
                }
                ws::Message::Text(msg) => {
                    log::info!("Text (len {})", msg.len());
                    let service = self.service.clone();
                    ctx.spawn(actix::fut::wrap_future(Self::handle(
                        service,
                        msg.into_bytes(),
                    )));
                }
                ws::Message::Continuation(msg) => {
                    todo!("NYI Continuation: {:?}", msg);
                }
                ws::Message::Close(msg) => {
                    log::info!("Close: {:?}", msg);
                }
                ws::Message::Ping(msg) => {
                    log::info!("Ping (len {})", msg.len());
                }
                any => todo!("NYI support of: {:?}", any),
            },
            Err(cause) => ctx.close(Some(CloseReason {
                code: ws::CloseCode::Error,
                description: Some(
                    json!({
                        "cause": cause.to_string()
                    })
                    .to_string(),
                ),
            })),
        };
    }
}
