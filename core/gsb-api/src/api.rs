use crate::services::{self, GsbServices, WsMessagesHandler};
use crate::GsbApiError;
use actix::{Actor, StreamHandler};
use actix_http::{
    ws::{CloseReason, ProtocolError},
    StatusCode,
};
use actix_web::web::{service, Data};
use actix_web::Scope;
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder, Result};
use actix_web_actors::ws;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::iter::FromIterator;
use std::sync::{Arc, Mutex};
use std::vec;

use ya_service_api_web::middleware::Identity;

pub const DEFAULT_SERVICES_TIMEOUT: f32 = 60.0;

pub fn web_scope() -> Scope {
    actix_web::web::scope(crate::GSB_API_PATH)
        .app_data(Data::new(crate::services::SERVICES.clone()))
        .service(post_services)
        .service(delete_services)
        .service(get_service_messages)
}

#[actix_web::post("/services")]
async fn post_services(
    query: web::Query<Timeout>,
    body: web::Json<ServicesBody>,
    id: Identity,
    services: Data<Arc<Mutex<GsbServices>>>,
) -> Result<impl Responder, GsbApiError> {
    log::debug!("POST /services Body: {:?}", body);
    if let Some(listen) = &body.listen {
        let components = listen.components.clone();
        let path = listen.on.clone();
        let mut services = services.lock()?;
        services.bind(HashSet::from_iter(components.iter().cloned()), path);
    }
    let services = ServicesBody {
        listen: Some(ServicesListenBody {
            on: "dummy".to_string(),
            components: vec!["foo".to_string(), "bar".to_string()],
            links: Some(ServicesLinksBody {
                messages: "gsb-api/v1/services/dummy/messages".to_string(),
            }),
        }),
    };
    Ok(web::Json(services)
        .customize()
        .with_status(StatusCode::CREATED))
}

#[actix_web::delete("/services/{key}")]
async fn delete_services(
    path: web::Path<ServicesPath>,
    id: Identity,
    services: Data<Arc<GsbServices>>,
) -> impl Responder {
    log::debug!("DELETE /services/{}", path.key);
    web::Json(())
}

#[actix_web::get("/services/{key}/messages")]
async fn get_service_messages(
    path: web::Path<ServicesPath>,
    req: HttpRequest,
    stream: web::Payload,
    services: Data<Arc<GsbServices>>, // id: Identity
) -> Result<HttpResponse, Error> {
    let services = services.as_ref().clone();
    let handler = WsMessagesHandler { services };
    let resp = ws::start(handler, &req, stream);
    println!("{:?}", resp);
    resp
}

#[derive(Deserialize)]
pub struct ServicesPath {
    pub key: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct ServicesBody {
    listen: Option<ServicesListenBody>,
}

#[derive(Deserialize, Serialize, Debug)]
struct ServicesListenBody {
    on: String,
    components: Vec<String>,
    links: Option<ServicesLinksBody>,
}

#[derive(Deserialize, Serialize, Debug)]
struct ServicesLinksBody {
    messages: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Timeout {
    #[serde(rename = "timeout", default = "default_services_timeout")]
    pub timeout: Option<f32>,
}

#[inline(always)]
pub(crate) fn default_services_timeout() -> Option<f32> {
    Some(DEFAULT_SERVICES_TIMEOUT)
}
