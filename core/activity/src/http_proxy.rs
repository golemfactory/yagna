use actix_http::Method;
use actix_web::web::Json;
use actix_web::{web, HttpRequest, HttpResponse};
use serde_json::{Map, Value};

use ya_client_model::market::Role;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::common::*;
use gsb_http_proxy::HttpToGsbProxy;
use ya_core_model::activity;
use ya_core_model::net::RemoteEndpoint;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .service(get_proxy_http_request)
        .service(post_proxy_http_request)
}

#[actix_web::get("/activity/{activity_id}/proxy_http_request{url:.*}")]
async fn get_proxy_http_request(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityUrl>,
    id: Identity,
    request: HttpRequest,
) -> Result<HttpResponse, actix_web::Error> {
    proxy_http_request(db, path, id, request, None, Method::GET).await
}

#[actix_web::post("/activity/{activity_id}/proxy_http_request{url:.*}")]
async fn post_proxy_http_request(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityUrl>,
    body: Json<Map<String, Value>>,
    id: Identity,
    request: HttpRequest,
) -> Result<HttpResponse, actix_web::Error> {
    proxy_http_request(db, path, id, request, Some(body.into_inner()), Method::POST).await
}

async fn proxy_http_request(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityUrl>,
    id: Identity,
    request: HttpRequest,
    body: Option<Map<String, Value>>,
    method: Method,
) -> Result<HttpResponse, actix_web::Error> {
    let path_activity_url = path.into_inner();
    let activity_id = path_activity_url.activity_id;
    let path = path_activity_url.url;

    // TODO: check if caller is the Requestor
    let result = authorize_activity_executor(&db, id.identity, &activity_id, Role::Provider).await;
    if let Err(e) = result {
        log::error!("Authorize error {}", e);
    }

    let agreement = get_activity_agreement(&db, &activity_id, Role::Requestor).await?;

    let gsb_call = HttpToGsbProxy {
        method: method.to_string(),
        path,
        body,
    };
    let stream = gsb_call.pass(move |msg| {
        let from = id.identity;
        let to = *agreement.provider_id();
        let bus_id = &activity::exeunit::bus_id(&activity_id);

        ya_net::from(from)
            .to(to)
            .service_transfer(bus_id)
            .call_streaming(msg)
    });

    Ok(HttpResponse::Ok()
        .keep_alive()
        .content_type(mime::TEXT_EVENT_STREAM.essence_str())
        .streaming(stream))
}
