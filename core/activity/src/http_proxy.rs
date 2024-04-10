use actix_http::Method;
use actix_web::http::header;
use actix_web::web::Bytes;
use actix_web::{web, Either, HttpRequest, HttpResponse, Responder};
use futures::{Stream, StreamExt};

use ya_client_model::market::Role;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::Error;

use crate::common::*;
use crate::error;
use ya_core_model::activity;
use ya_core_model::net::RemoteEndpoint;
use ya_gsb_http_proxy::http_to_gsb::HttpToGsbProxy;

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
    // ) -> Result<HttpResponse, actix_web::Error> {
) -> crate::Result<impl Responder> {
    proxy_http_request(db, path, id, request, None, Method::GET).await
}

#[actix_web::post("/activity/{activity_id}/proxy_http_request{url:.*}")]
async fn post_proxy_http_request(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityUrl>,
    body: web::Bytes,
    id: Identity,
    request: HttpRequest,
    // ) -> Result<HttpResponse, actix_web::Error> {
) -> crate::Result<impl Responder> {
    proxy_http_request(db, path, id, request, Some(body), Method::POST).await
}

async fn proxy_http_request(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityUrl>,
    id: Identity,
    request: HttpRequest,
    body: Option<web::Bytes>,
    method: Method,
) -> crate::Result<impl Responder> {
    let path_activity_url = path.into_inner();
    let activity_id = path_activity_url.activity_id;
    let path = path_activity_url.url;

    // TODO: check if caller is the Requestor
    let result = authorize_activity_executor(&db, id.identity, &activity_id, Role::Requestor).await;
    if let Err(e) = result {
        log::error!("Authorize error {}", e);
    }

    let agreement = get_activity_agreement(&db, &activity_id, Role::Requestor).await?;

    let http_to_gsb = HttpToGsbProxy {
        method: method.to_string(),
        path,
        body: body.map(|bytes| bytes.to_vec()),
        headers: request.headers().clone(),
    };

    let stream = http_to_gsb.pass(move |msg| {
        let from = id.identity;
        let to = *agreement.provider_id();
        let bus_id = &activity::exeunit::bus_id(&activity_id);

        ya_net::from(from)
            .to(to)
            .service_transfer(bus_id)
            .call_streaming(msg)
    });

    if let Some(value) = request.headers().get(header::ACCEPT) {
        if value.eq(mime::TEXT_EVENT_STREAM.essence_str()) {
            return Ok(Either::Left(stream_results(stream)?));
        }
    }
    Ok(Either::Right(await_results(stream).await?))
}

fn stream_results(
    stream: impl Stream<Item = Result<Bytes, Error>> + Unpin + 'static,
) -> crate::Result<impl Responder> {
    Ok(HttpResponse::Ok()
        .keep_alive()
        .content_type(mime::TEXT_EVENT_STREAM.essence_str())
        .streaming(stream))
}

async fn await_results(
    mut stream: impl Stream<Item = Result<Bytes, Error>> + Unpin,
) -> crate::Result<impl Responder> {
    let response = stream.next().await;

    if let Some(Ok(bytes)) = response {
        let response_body = String::from_utf8(bytes.to_vec())
            .map_err(|e| error::Error::Service(format!("Conversion from utf8 failed {e}")))?;
        return Ok(HttpResponse::Ok().body(response_body));
    }
    Ok(HttpResponse::InternalServerError().body("No response"))
}