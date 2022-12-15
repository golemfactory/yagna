//! Provider side operations
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    future::Future,
    marker::PhantomData,
    sync::{Arc, Mutex},
    vec,
};

use actix::{Actor, StreamHandler};
use actix_http::{
    ws::{CloseReason, Item, ProtocolError},
    StatusCode,
};
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder};
use actix_web_actors::ws;
use lazy_static::lazy_static;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use serde_json::json;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use crate::{GsbApiError, WsRequest, WsResponse, WS_CALL};

lazy_static! {
    pub(crate) static ref SERVICES: Arc<Mutex<GsbServices>> =
        Arc::new(Mutex::new(GsbServices::default()));
}

// type MSG = Box<dyn RpcMessage<Item = Box<dyn Serialize + 'static + Sync + Send>, Error = Box<dyn Serialize + 'static + Sync + Send>>>;
// // type CALLER = ;
// type MSG_FUT = Future<Output = Result<MSG, anyhow::Error>>;

// // trait Caller: FnMut(String, MSG) -> Future<Output = Result<MSG, anyhow::Error>> + 'static {}
// type CALLER = Box<dyn FnMut(String, MSG) -> MSG_FUT>;

#[derive(Default)]
struct GsbCaller<REQ, RES>
where
    REQ: RpcMessage + Into<WsRequest>,
    RES: RpcMessage + From<WsResponse>,
{
    req_type: PhantomData<REQ>,
    res_type: PhantomData<RES>,
    ws_caller: Option<Arc<Mutex<WS_CALL>>>,
}

impl<REQ, RES> GsbCaller<REQ, RES>
where
    REQ: RpcMessage + Into<WsRequest>,
    RES: RpcMessage + From<WsResponse>,
{
    async fn call(path: String, req: REQ) -> Result<REQ, REQ::Error> {
        todo!("NYI")
    }
}

struct GsbMessage;

#[derive(Default)]
pub(crate) struct GsbServices {
    callers: HashMap<String, Vec<WS_CALL>>,
}

impl GsbServices {
    pub fn bind(&mut self, components: HashSet<String>, path: String) -> Result<(), GsbApiError> {
        for component in components {
            match component.as_str() {
                "GetMetadata" => {
                    log::info!("GetMetadata {path}");
                }
                "GetChunk" => {
                    log::info!("GetChunk {path}");
                }
                _ => return Err(GsbApiError::BadRequest),
            }
        }
        Ok(())
    }

    pub fn bind_service<T: RpcMessage>(id: String, path: String) {
        let caller = bus::bind_with_caller(&path, move |caller, packet: T| {});
        todo!()
    }
}

pub(crate) struct WsMessagesHandler {
    pub services: Arc<Mutex<GsbServices>>,
}

impl Actor for WsMessagesHandler {
    type Context = ws::WebsocketContext<Self>;
}

impl StreamHandler<Result<actix_http::ws::Message, ProtocolError>> for WsMessagesHandler {
    fn handle(
        &mut self,
        item: Result<actix_http::ws::Message, ProtocolError>,
        ctx: &mut Self::Context,
    ) {
        match item {
            Ok(msg) => {
                match msg {
                    ws::Message::Text(msg) => {
                        log::info!("Text: {:?}", msg);
                        match serde_json::from_slice::<WsRequest>(msg.as_bytes()) {
                            Ok(request) => {
                                log::info!("WsRequest: {request:?}");
                            }
                            Err(err) => todo!("NYI Deserialization error: {err:?}"),
                        }
                    }
                    ws::Message::Binary(msg) => {
                        todo!("NYI Binary: {:?}", msg);
                    }
                    ws::Message::Continuation(msg) => {
                        todo!("NYI Continuation: {:?}", msg);
                    }
                    ws::Message::Close(msg) => {
                        log::info!("Close: {:?}", msg);
                    }
                    ws::Message::Ping(msg) => {
                        log::info!("Ping: {:?}", msg);
                    }
                    any => todo!("NYI support of: {:?}", any),
                }
                ctx.text(
                    json!({
                      "id": "myId",
                      "component": "GetMetadata",
                      "payload": "payload",
                    })
                    .to_string(),
                )
            }
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
