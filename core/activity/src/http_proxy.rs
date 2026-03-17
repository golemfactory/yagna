use actix_http::header::HeaderValue;
use actix_http::{header, StatusCode};
use actix_web::web::Bytes;
use actix_web::{web, Either, HttpRequest, HttpResponse, Responder};
use futures::Stream;

use ya_client_model::market::Role;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::Error;

use crate::common::*;
use crate::error;
use ya_core_model::activity;
use ya_gsb_http_proxy::http_to_gsb::BindingMode::Net;
use ya_gsb_http_proxy::http_to_gsb::{
    HttpToGsbProxy, HttpToGsbProxyResponse, HttpToGsbProxyStreamingResponse, NetBindingNodes,
};

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope.service(proxy_http_request)
}

#[actix_web::route(
    "/activity/{activity_id}/proxy-http/{url:.*}",
    method = "GET",
    method = "POST",
    method = "PUT",
    method = "DELETE",
    method = "HEAD",
    method = "OPTIONS",
    method = "CONNECT",
    method = "PATCH",
    method = "TRACE"
)]
async fn proxy_http_request(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityUrl>,
    id: Identity,
    request: HttpRequest,
    body: Option<web::Bytes>,
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

    let method = request.method().to_string();
    let body = body.map(|bytes| bytes.to_vec());
    let headers = request.headers().clone();

    if let Some(accept_header) = request.headers().get(header::ACCEPT) {
        let accept_header = accept_header
            .to_str()
            .map_err(|e| error::Error::BadRequest(format!("Invalid accept header: {e}")))?;
        if accept_header.eq(mime::TEXT_EVENT_STREAM.essence_str())
            || accept_header.eq(mime::APPLICATION_OCTET_STREAM.essence_str())
        {
            let accept_header_value = HeaderValue::from_str(accept_header).map_err(|e| {
                error::Error::BadRequest(format!("Invalid accept header value: {e}"))
            })?;
            let response = http_to_gsb
                .pass_streaming(method, path, headers, body)
                .await;

            return Ok(Either::Left(
                stream_results(response, accept_header_value).await?,
            ));
        }
    }
    let response = http_to_gsb.pass(method, path, headers, body).await;
    Ok(Either::Right(build_response(response).await?))
}

async fn stream_results(
    response: HttpToGsbProxyStreamingResponse<
        impl Stream<Item = Result<Bytes, Error>> + Unpin + 'static,
    >,
    content_type: HeaderValue,
) -> crate::Result<impl Responder> {
    let mut response_builder = HttpResponse::build(
        StatusCode::from_u16(response.status_code)
            .map_err(|e| error::Error::Service(format!("Invalid status code {e}")))?,
    );
    for (h, vals) in response.response_headers {
        for v in vals {
            response_builder.append_header((h.as_str(), v));
        }
    }
    match response.body {
        Ok(body) => Ok(response_builder
            .keep_alive()
            .content_type(content_type)
            .streaming(body)),
        Err(err) => {
            let reason = format!("{err}");
            Ok(response_builder.body(reason))
        }
    }
}

async fn build_response(
    response: HttpToGsbProxyResponse<Result<Bytes, Error>>,
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
