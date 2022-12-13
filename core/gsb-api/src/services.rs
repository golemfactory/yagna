//! Provider side operations
use std::{vec, sync::Arc};

use actix::{Actor, StreamHandler};
use actix_http::{
    ws::{CloseReason, ProtocolError},
    StatusCode,
};
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder};
use actix_web_actors::ws;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};

use serde_json::json;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;


lazy_static! {
    pub(crate) static ref SERVICES: Arc<Services> = Arc::new(Services {});
}

pub(crate) struct Services;

pub(crate) struct WsMessagesHandler {
    pub services: Arc<Services>
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
