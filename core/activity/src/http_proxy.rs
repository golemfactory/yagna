use actix_http::{Method, StatusCode};
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
use ya_gsb_http_proxy::http_to_gsb::BindingMode::Net;
use ya_gsb_http_proxy::http_to_gsb::{HttpToGsbProxy, HttpToGsbProxyResponse, NetBindingNodes};

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .service(get_proxy_http_request)
        .service(post_proxy_http_request)
}

#[actix_web::get("/activity/{activity_id}/proxy-http/{url:.*}")]
async fn get_proxy_http_request(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityUrl>,
    id: Identity,
    request: HttpRequest,
) -> crate::Result<impl Responder> {
    proxy_http_request(db, path, id, request, None, Method::GET).await
}

#[actix_web::post("/activity/{activity_id}/proxy-http/{url:.*}")]
async fn post_proxy_http_request(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityUrl>,
    body: web::Bytes,
    id: Identity,
    request: HttpRequest,
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

    let result = authorize_activity_executor(&db, id.identity, &activity_id, Role::Requestor).await;
    if let Err(e) = result {
        log::error!("Authorize error {}", e);
    }

    let agreement = get_activity_agreement(&db, &activity_id, Role::Requestor).await?;

    let mut http_to_gsb = HttpToGsbProxy::new(Net(NetBindingNodes {
        from: id.identity,
        to: *agreement.provider_id(),
    }))
    .bus_addr(&activity::exeunit::bus_id(&activity_id));

    let method = method.to_string();
    let body = body.map(|bytes| bytes.to_vec());
    let headers = request.headers().clone();

    if let Some(accept_header) = request.headers().get(header::ACCEPT) {
        if accept_header.eq(mime::TEXT_EVENT_STREAM.essence_str())
            || accept_header.eq(mime::APPLICATION_OCTET_STREAM.essence_str())
        {
            let stream = http_to_gsb
                .pass_streaming(method, path, headers, body)
                .await;
            return Ok(Either::Left(
                stream_results(stream, accept_header.to_str().unwrap()).await?,
            ));
        }
    }
    let response = http_to_gsb.pass(method, path, headers, body).await;
    Ok(Either::Right(build_response(response).await?))
}

async fn stream_results(
    stream: impl Stream<Item = HttpToGsbProxyResponse<Result<Bytes, Error>>> + Unpin + 'static,
    content_type: &str,
) -> crate::Result<impl Responder> {
    Ok(HttpResponse::Ok()
        .keep_alive()
        .content_type(content_type)
        .streaming(stream.map(|e| e.body)))
}

async fn build_response(
    mut response: HttpToGsbProxyResponse<Result<Bytes, Error>>,
) -> crate::Result<impl Responder> {
    if let Ok(bytes) = response.body {
        let response_body = String::from_utf8(bytes.to_vec())
            .map_err(|e| error::Error::Service(format!("Conversion from utf8 failed {e}")))?;

        let mut response_builder = HttpResponse::build(
            StatusCode::from_u16(response.status_code)
                .map_err(|e| error::Error::Service(format!("Invalid status code {e}")))?,
        );
        for (h, vals) in response.response_headers {
            for v in vals {
                response_builder.append_header((h.as_str(), v));
            }
        }
        return Ok(response_builder.body(response_body));
    }

    Ok(HttpResponse::InternalServerError().body("No response"))
}
