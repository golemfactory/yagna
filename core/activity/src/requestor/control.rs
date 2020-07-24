use actix_rt::Arbiter;
use actix_web::http::header;
use actix_web::{web, Either, HttpRequest, HttpResponse, Responder};
use bytes::{BufMut, Bytes, BytesMut};
use futures::{FutureExt, StreamExt, TryFutureExt};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ya_client_model::activity::{
    ActivityState, ExeScriptCommand, ExeScriptRequest, RuntimeEvent, RuntimeEventKind, State,
};
use ya_client_model::market::Agreement;
use ya_core_model::activity;
use ya_net::{self as net, RemoteEndpoint};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{timeout::IntoTimeoutFuture, RpcEndpoint};

use crate::common::*;
use crate::dao::{ActivityDao, RuntimeEventDao};
use crate::error::Error;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .service(create_activity)
        .service(destroy_activity)
        .service(exec)
        .service(get_batch_results)
}

/// Creates new Activity based on given Agreement.
#[actix_web::post("/activity")]
async fn create_activity(
    db: web::Data<DbExecutor>,
    query: web::Query<QueryTimeout>,
    body: web::Json<String>,
    id: Identity,
) -> impl Responder {
    let agreement_id = body.into_inner();
    authorize_agreement_initiator(id.identity, &agreement_id).await?;

    let agreement = get_agreement(&agreement_id).await?;
    log::trace!("agreement: {:#?}", agreement);

    let provider_id = agreement.provider_id()?.parse()?;
    let msg = activity::Create {
        provider_id,
        agreement_id: agreement_id.clone(),
        timeout: query.timeout.clone(),
    };

    let activity_id = net::from(id.identity)
        .to(provider_id)
        .service(activity::BUS_ID)
        .send(msg)
        .timeout(query.timeout)
        .await???;

    log::debug!("activity created: {}, inserting", activity_id);
    db.as_dao::<ActivityDao>()
        .create_if_not_exists(&activity_id, &agreement_id)
        .await?;

    Ok::<_, Error>(web::Json(activity_id))
}

/// Destroys given Activity.
#[actix_web::delete("/activity/{activity_id}")]
async fn destroy_activity(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let agreement = get_activity_agreement(&db, &path.activity_id).await?;
    let msg = activity::Destroy {
        activity_id: path.activity_id.to_string(),
        agreement_id: agreement.agreement_id.clone(),
        timeout: query.timeout.clone(),
    };
    agreement_provider_service(&id, &agreement)?
        .send(msg)
        .timeout(query.timeout)
        .await???;

    set_persisted_state(
        &db,
        &path.activity_id,
        ActivityState {
            state: State::Terminated.into(),
            reason: None,
            error_message: None,
        },
    )
    .await
    .map(|_| web::Json(()))
}

/// Executes an ExeScript batch within a given Activity.
#[actix_web::post("/activity/{activity_id}/exec")]
async fn exec(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    body: web::Json<ExeScriptRequest>,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let commands: Vec<ExeScriptCommand> =
        serde_json::from_str(&body.text).map_err(|e| Error::BadRequest(format!("{:?}", e)))?;
    let agreement = get_activity_agreement(&db, &path.activity_id).await?;
    let batch_id = generate_id();
    let msg = activity::Exec {
        activity_id: path.activity_id.clone(),
        batch_id: batch_id.clone(),
        exe_script: commands,
        timeout: query.timeout.clone(),
    };

    ya_net::from(id.identity)
        .to(agreement.provider_id()?.parse()?)
        .service(&activity::exeunit::bus_id(&path.activity_id))
        .send(msg)
        .timeout(query.timeout)
        .await???;

    Ok::<_, Error>(web::Json(batch_id))
}

/// Queries for ExeScript batch results.
#[actix_web::get("/activity/{activity_id}/exec/{batch_id}")]
async fn get_batch_results(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityBatch>,
    query: web::Query<QueryTimeoutCommandIndex>,
    id: Identity,
    request: HttpRequest,
) -> Result<impl Responder, Error> {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;
    let agreement = get_activity_agreement(&db, &path.activity_id).await?;

    if let Some(value) = request.headers().get(header::ACCEPT) {
        if value.eq(mime::TEXT_EVENT_STREAM.essence_str()) {
            let db = db.get_ref().clone();
            return Ok(Either::A(stream_results(db, agreement, path, id)?));
        }
    }
    Ok(Either::B(await_results(agreement, path, query, id).await?))
}

async fn await_results(
    agreement: Agreement,
    path: web::Path<PathActivityBatch>,
    query: web::Query<QueryTimeoutCommandIndex>,
    id: Identity,
) -> Result<impl Responder, Error> {
    let msg = activity::GetExecBatchResults {
        activity_id: path.activity_id.to_string(),
        batch_id: path.batch_id.to_string(),
        timeout: query.timeout,
        command_index: query.command_index,
    };

    let results = ya_net::from(id.identity)
        .to(agreement.provider_id()?.parse()?)
        .service(&activity::exeunit::bus_id(&path.activity_id))
        .send(msg)
        .timeout(query.timeout)
        .await???;

    Ok::<_, Error>(web::Json(results))
}

fn stream_results(
    db: DbExecutor,
    agreement: Agreement,
    path: web::Path<PathActivityBatch>,
    id: Identity,
) -> Result<impl Responder, Error> {
    let msg = activity::StreamExecBatchResults {
        activity_id: path.activity_id.to_string(),
        batch_id: path.batch_id.to_string(),
    };

    let seq = AtomicU64::new(0);
    let stream = ya_net::from(id.identity)
        .to(agreement.provider_id()?.parse()?)
        .service(&activity::exeunit::bus_id(&path.activity_id))
        .call_streaming(msg)
        .inspect(move |entry| match entry {
            Ok(Ok(evt)) => persist_event(&db, &path.activity_id, &evt),
            _ => (),
        })
        .map(|item| match item {
            Ok(result) => result.map_err(Error::from),
            Err(e) => Err(Error::from(e)),
        })
        .map(Either::A)
        .chain(tokio::time::interval(Duration::from_secs(15)).map(Either::B))
        .map(move |e| match e {
            Either::A(r) => map_event_result(r, seq.fetch_add(1, Ordering::Relaxed)),
            Either::B(_) => Ok(Bytes::from_static(":ping\n".as_bytes())),
        });

    Ok(HttpResponse::Ok()
        .content_type(mime::EVENT_STREAM.as_str())
        .streaming(stream))
}

fn persist_event(db: &DbExecutor, activity_id: &String, event: &RuntimeEvent) {
    match &event.kind {
        RuntimeEventKind::StdOut(_) | RuntimeEventKind::StdErr(_) => (),
        _ => {
            let db = db.clone();
            let activity_id = activity_id.clone();
            let event = event.clone();

            let fut = async move {
                db.as_dao::<RuntimeEventDao>()
                    .create(&activity_id, event)
                    .map_err(|e| log::warn!("Cannot persist event: {:?}", e))
                    .map(|_| ())
                    .await;
            };
            Arbiter::spawn(fut)
        }
    }
}

fn map_event_result<T: Serialize>(
    result: Result<T, Error>,
    id: u64,
) -> Result<Bytes, actix_web::Error> {
    let json = serde_json::to_string(&result?).map_err(|e| Error::Service(e.to_string()))?;
    let mut bytes = BytesMut::with_capacity(128);
    bytes.put_slice(b"event: runtime");
    bytes.put_slice(b"\ndata: ");
    bytes.put_slice(json.as_bytes());
    bytes.put_slice(b"\nid: ");
    bytes.put_slice(id.to_string().as_bytes());
    bytes.put_slice(b"\n\n");
    Ok(bytes.freeze())
}

#[derive(Deserialize)]
struct PathActivityBatch {
    activity_id: String,
    batch_id: String,
}
