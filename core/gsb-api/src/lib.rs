mod api;
mod services;

use actix::{dev::MessageResponse, Actor, Handler, StreamHandler};
use actix_http::{
    ws::{CloseCode, CloseReason, ProtocolError},
    StatusCode,
};
use actix_web::ResponseError;
use actix_web_actors::ws;
use bytes::Bytes;
use flexbuffers::Reader;
use futures::channel::oneshot::Sender;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use services::GsbServices;
use std::{
    collections::HashMap,
    sync::{Arc, MutexGuard, PoisonError, RwLock},
};
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
    InternalError,
    #[error(transparent)]
    Any(#[from] anyhow::Error),
}

impl From<PoisonError<MutexGuard<'_, GsbServices>>> for GsbApiError {
    fn from(_value: PoisonError<MutexGuard<'_, GsbServices>>) -> Self {
        GsbApiError::InternalError
    }
}

impl From<serde_json::Error> for GsbApiError {
    fn from(_value: serde_json::Error) -> Self {
        GsbApiError::InternalError
    }
}

impl From<actix_web::Error> for GsbApiError {
    fn from(_value: actix_web::Error) -> Self {
        GsbApiError::InternalError
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
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct WsRequest {
    id: String,
    component: String,
    msg: Vec<u8>,
}

impl actix::Message for WsRequest {
    type Result = WsResult;
}

#[derive(Debug)]
struct WsResponse {
    id: String,
    msg: Vec<u8>,
}

type WsResult = Result<WsResponse, anyhow::Error>;

impl MessageResponse<WsMessagesHandler, WsRequest> for Result<(), WsApiError> {
    fn handle(
        self,
        ctx: &mut <WsMessagesHandler as Actor>::Context,
        tx: Option<actix::dev::OneshotSender<<WsRequest as actix::Message>::Result>>,
    ) {
        match self {
            Ok(()) => {}
            Err(err) => ctx.close(Some(CloseReason {
                code: CloseCode::Error,
                description: Some(err.to_string()),
            })),
        }
    }
}

pub(crate) struct WsMessagesHandler {
    pub responders: Arc<RwLock<HashMap<String, Sender<WsResult>>>>,
}

impl WsMessagesHandler {
    fn handle(&mut self, msg: bytes::Bytes) {
        match Reader::get_root(&*msg) {
            Ok(buffer) => {
                let response = buffer.as_map();
                //TODO handle errors
                let id = response.index("id").unwrap().as_str().to_string();
                let msg = msg.to_vec();
                let response = WsResponse { id, msg };
                log::info!("WsResponse: {} len: {}", response.id, response.msg.len());
                let mut responders = self.responders.write().unwrap();
                match responders.remove(&response.id) {
                    Some(responder) => {
                        if let Err(err) = responder.send(Ok(response)) {
                            log::error!("Failed to handle msg");
                        }
                    }
                    None => {
                        log::error!("No matching response id");
                    }
                }
            }
            Err(err) => {
                log::error!("Failed to deserialize msg: {}", err);
            }
        }
    }
}

impl Actor for WsMessagesHandler {
    type Context = ws::WebsocketContext<Self>;
}

impl Handler<WsRequest> for WsMessagesHandler {
    type Result = Result<(), WsApiError>;

    fn handle(&mut self, msg: WsRequest, ctx: &mut Self::Context) -> Self::Result {
        let msg = flexbuffers::to_vec(&msg)
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
                ws::Message::Text(msg) => {
                    log::info!("Text (len {})", msg.len());
                    self.handle(msg.into_bytes());
                }
                ws::Message::Binary(msg) => {
                    log::info!("Binary (len {})", msg.len());
                    self.handle(msg);
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
