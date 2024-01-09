use actix_web::http::header;
use actix_web::web::{Bytes, BytesMut};
use actix_web::{web, Either, HttpRequest, HttpResponse};
use futures::prelude::*;
use std::time::Duration;
use tokio_stream::wrappers::IntervalStream;

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
        // .service(get_activities_web)
        .service(proxy_http_request)
}

#[actix_web::get("/activity/{activity_id}/proxy_http_request")]
async fn proxy_http_request(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    _query: web::Query<QueryTimeout>,
    id: Identity,
    request: HttpRequest,
) -> Result<HttpResponse, actix_web::Error> {
    // // check if caller is the Requestor
    // let result =
    //     authorize_activity_executor(&db, id.identity, &path.activity_id, Role::Requestor).await;
    // if let Err(e) = result {
    //     return Err(e);
    // }

    let agreement = get_activity_agreement(&db, &path.activity_id, Role::Requestor).await?;

    if let Some(value) = request.headers().get(header::ACCEPT) {
        log::info!("[Header]: {:?}", value);
        // if value.eq(mime::TEXT_EVENT_STREAM.essence_str()) {
        // return Ok(Either::Left(crate::requestor::control::stream_results(agreement, path, id)?));
        // }
    }
    let msg = GsbHttpCall {
        host: "http://localhost".to_string(),
    };

    let stream = ya_net::from(id.identity)
        .to(*agreement.provider_id())
        .service_transfer(&activity::exeunit::bus_id(&path.activity_id))
        .call_streaming(msg);

    // stream
    //     .for_each(|r| async move {
    //         match r {
    //             Ok(r) => match r {
    //                 Ok(r) => {
    //                     let msg = format!("[STREAM exeu #{}][{}] {}", r.index, r.timestamp, r.val);
    //                     log::info!("{}", msg);
    //                 }
    //                 Err(e) => {
    //                     log::error!("error {}", e);
    //                 }
    //             },
    //             Err(e) => {
    //                 log::error!("error {}", e);
    //             }
    //         }
    //     })
    //     .await;

    let stream = stream
        .map(|item| match item {
            Ok(result) => result.map_err(|e| Error::BadRequest(e.to_string())),
            Err(e) => Err(Error::from(e)),
        })
        .map(Either::Left)
        .chain({
            let interval = tokio::time::interval(Duration::from_secs(15));
            IntervalStream::new(interval).map(Either::Right)
        })
        .map(move |e| match e {
            Either::Left(r) => {
                let mut bytes = BytesMut::new();
                let msg = match r {
                    Ok(r) => format!("[STREAM exeu #{}][{}] {}", r.index, r.timestamp, r.val),
                    Err(e) => format!("Error {}", e),
                };
                bytes.extend_from_slice(msg.as_bytes());
                Ok::<Bytes, actix_web::Error>(bytes.freeze())
            }
            Either::Right(_) => Ok(Bytes::from_static(":ping\n".as_bytes())),
        });

    Ok(HttpResponse::Ok()
        .keep_alive()
        .content_type(mime::TEXT_EVENT_STREAM.essence_str())
        // .content_type(mime::APPLICATION_OCTET_STREAM.essence_str())
        .streaming(stream))
}
