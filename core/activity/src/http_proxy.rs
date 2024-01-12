use actix_http::Method;
use actix_web::http::header;
use actix_web::web::{Bytes, BytesMut, Json};
use actix_web::{web, HttpRequest, HttpResponse};
use futures::prelude::*;
use serde_json::{Map, Value};

use ya_client_model::market::Role;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::common::*;
use crate::error::Error;
use gsb_http_proxy::GsbHttpCall;
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

    // TODO: check if caller is the Requestor
    let result = authorize_activity_executor(&db, id.identity, &activity_id, Role::Provider).await;
    if let Err(e) = result {
        log::error!("Authorize error {}", e);
    }

    let agreement = get_activity_agreement(&db, &activity_id, Role::Requestor).await?;

    // TODO: take care of headers
    if let Some(value) = request.headers().get(header::ACCEPT) {
        log::info!("[Header]: {:?}", value);
    }

    let stream_fun = |msg, provider_id, activity_id| {
        ya_net::from(id.identity)
            .to(provider_id)
            .service_transfer(&activity::exeunit::bus_id(activity_id))
            .call_streaming(msg)
    };

    let path = if path_activity_url.url.starts_with('/') {
        path_activity_url.url[1..].to_string()
    } else {
        path_activity_url.url
    };

    let msg = GsbHttpCall {
        method: method.to_string(),
        path,
        body,
    };

    let stream = stream_fun(msg, *agreement.provider_id(), &activity_id);

    let stream = stream
        .map(|item| match item {
            Ok(result) => result.map_err(|e| Error::BadRequest(e.to_string())),
            Err(e) => Err(Error::from(e)),
        })
        .map(move |result| {
            let mut bytes = BytesMut::new();
            let msg = match result {
                Ok(r) => r.msg,
                Err(e) => format!("Error {}", e),
            };
            bytes.extend_from_slice(msg.as_bytes());
            Ok::<Bytes, actix_web::Error>(bytes.freeze())
        });

    Ok(HttpResponse::Ok()
        .keep_alive()
        .content_type(mime::TEXT_EVENT_STREAM.essence_str())
        .streaming(stream))
}
