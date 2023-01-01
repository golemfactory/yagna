use crate::services::GsbServices;
use crate::{GsbApiError, WsMessagesHandler};
use actix_http::StatusCode;
use actix_web::web::Data;
use actix_web::Scope;
use actix_web::{web, HttpRequest, Responder, Result};
use actix_web_actors::ws::{self};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
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
    _query: web::Query<Timeout>,
    body: web::Json<ServicesBody>,
    _id: Identity,
    services: Data<Arc<Mutex<GsbServices>>>,
) -> Result<impl Responder, GsbApiError> {
    log::debug!("POST /services Body: {:?}", body);
    if let Some(listen) = &body.listen {
        let components = listen.components.clone();
        let listen_on = listen.on.clone();
        let mut services = services.lock()?;
        let _ = services.bind(components.iter().map(String::as_str).collect(), &listen_on)?;
        let listen_on_encoded = base64::encode(&listen_on);
        let services = ServicesBody {
            listen: Some(ServicesListenBody {
                on: listen_on,
                components: components,
                links: Some(ServicesLinksBody {
                    messages: format!("gsb-api/v1/services/{listen_on_encoded}"),
                }),
            }),
        };
        return Ok(web::Json(services)
            .customize()
            .with_status(StatusCode::CREATED));
    }
    Err(GsbApiError::BadRequest)
}

#[actix_web::delete("/services/{key}")]
async fn delete_services(
    path: web::Path<ServicesPath>,
    _id: Identity,
    _services: Data<Arc<Mutex<GsbServices>>>,
) -> impl Responder {
    log::debug!("DELETE /services/{}", path.key);
    web::Json(())
}

#[actix_web::get("/services/{key}")]
async fn get_service_messages(
    path: web::Path<ServicesPath>,
    req: HttpRequest,
    stream: web::Payload,
    services: Data<Arc<Mutex<GsbServices>>>,
) -> Result<impl Responder, GsbApiError> {
    let mut services = services.lock()?;
    //TODO handle decode error
    let key = base64::decode(&path.key).unwrap();
    let responders = services.ws_responses_dst(&String::from_utf8_lossy(&key));
    let responders = responders.clone();
    let handler = WsMessagesHandler { responders };
    let (addr, resp) = ws::WsResponseBuilder::new(handler, &req, stream).start_with_addr()?;
    let ws_request_dst = services.ws_request_dst(&path.key);
    let mut ws_request_dst = ws_request_dst.write().unwrap();
    *ws_request_dst = Some(addr);
    Ok(resp)
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
