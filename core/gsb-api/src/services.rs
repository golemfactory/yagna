//! Provider side operations
use std::vec;

use actix_http::StatusCode;
use actix_web::{web, Responder};
use serde::{Deserialize, Serialize};

use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

pub const DEFAULT_SERVICES_TIMEOUT: f32 = 60.0;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope.service(post_services)
}

#[actix_web::post("/services")]
async fn post_services(
    query: web::Query<Timeout>,
    body: web::Json<ServicesBody>,
    id: Identity,
) -> impl Responder {
    log::debug!("POST /services Body: {:?}", body);
    let services = ServicesBody { 
            listen: Some(ServicesListenBody {
                on: "dummy".to_string(),
                components: vec!["foo".to_string(), "bar".to_string()],
                links: Some(ServicesLinksBody {
                    messages: "gsb-api/v1/services/dummy".to_string(),
                })
            })
        };
    web::Json(services)
        .customize()
        .with_status(StatusCode::CREATED)
}

#[actix_web::delete("/services/{key}")]
async fn delete_services(path: web::Path<ServicesPath>, id: Identity) -> impl Responder {
    log::debug!("DELETE /services/{}", path.key);
    web::Json(())
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
