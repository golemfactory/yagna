//! Provider side operations
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    future::Future,
    sync::Arc,
    vec,
};

use actix::{Actor, StreamHandler};
use actix_http::{
    ws::{CloseReason, ProtocolError},
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

use crate::GsbApiError;

lazy_static! {
    pub(crate) static ref SERVICES: Arc<GsbServices> = Arc::new(GsbServices::default());
}

struct WsRequest {
    id: String,
    component: String,
    payload: Vec<u8>,
}

struct WsResponse {
    id: String,
    payload: Vec<u8>,
}

type WS_CALL = Box<
    dyn FnMut(String, WsRequest) -> Future<Output = Result<WsResponse, anyhow::Error>>
        + Sync
        + Send,
>;

// type MSG = Box<dyn RpcMessage<Item = Box<dyn Serialize + 'static + Sync + Send>, Error = Box<dyn Serialize + 'static + Sync + Send>>>;
// // type CALLER = ;
// type MSG_FUT = Future<Output = Result<MSG, anyhow::Error>>;

// // trait Caller: FnMut(String, MSG) -> Future<Output = Result<MSG, anyhow::Error>> + 'static {}
// type CALLER = Box<dyn FnMut(String, MSG) -> MSG_FUT>;

struct GsbMessage;

#[derive(Default)]
pub(crate) struct GsbServices {
    callers: HashMap<String, Vec<WS_CALL>>,
}

impl GsbServices {
    pub fn bind(&mut self, components: HashSet<String>, path: String) -> Result<(), GsbApiError> {
        for component in components {
            match component.as_str() {
                "GetMetadata" => todo!(),
                "GetChunk" => todo!(),
                _ => return Err(GsbApiError::BadRequest),
            }
        }
        Ok(())
    }

    pub fn bind_service<T: RpcMessage>(id: String, path: String) {
        // bus::bind_with_caller(addr, f)
        todo!()
    }
}

pub(crate) struct WsMessagesHandler {
    pub services: Arc<GsbServices>,
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
            Ok(msg) => ctx.text(
                json!({
                  "id": "myId",
                  "component": "GetMetadata",
                  "payload": "payload",
                })
                .to_string(),
            ),
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
