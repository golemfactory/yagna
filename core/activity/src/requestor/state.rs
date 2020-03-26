use actix_web::{web, Responder};

use ya_core_model::activity;
use ya_net::TryRemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{timeout::IntoTimeoutFuture, RpcEndpoint};

use crate::common::{
    authorize_activity_initiator, get_activity_agreement, get_persisted_state, get_persisted_usage,
    set_persisted_state, set_persisted_usage, PathActivity, QueryTimeout,
};
use crate::error::Error;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .service(get_activity_state)
        .service(get_activity_usage)
        .service(get_running_command)
}

/// Get state of specified Activity.
#[actix_web::get("/activity/{activity_id}/state")]
async fn get_activity_state(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    // Return locally persisted usage if activity has been already terminated or terminating
    let state = get_persisted_state(&db, &path.activity_id).await?;
    if !state.alive() {
        return Ok(web::Json(state));
    }

    // Retrieve and persist activity state
    let agreement = get_activity_agreement(&db, &path.activity_id).await?;
    let provider_service = agreement.provider_id()?.try_service(activity::BUS_ID)?;
    let state = provider_service
        .send(activity::GetState {
            activity_id: path.activity_id.to_string(),
            timeout: query.timeout.clone(),
        })
        .timeout(query.timeout)
        .await???;

    set_persisted_state(&db, &path.activity_id, state.clone())
        .await
        .map(|_| web::Json(state))
}

/// Get usage of specified Activity.
#[actix_web::get("/activity/{activity_id}/usage")]
async fn get_activity_usage(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    // Return locally persisted usage if activity has been already terminated or terminating
    if get_persisted_state(&db, &path.activity_id).await?.alive() {
        return Ok(web::Json(
            get_persisted_usage(&db, &path.activity_id).await?,
        ));
    }

    // Retrieve and persist activity usage
    let agreement = get_activity_agreement(&db, &path.activity_id).await?;
    let provider_service = agreement.provider_id()?.try_service(activity::BUS_ID)?;
    let usage = provider_service
        .send(activity::GetUsage {
            activity_id: path.activity_id.to_string(),
            timeout: query.timeout.clone(),
        })
        .timeout(query.timeout)
        .await???;

    set_persisted_usage(&db, &path.activity_id, usage)
        .await
        .map(web::Json)
}

/// Get running command for a specified Activity.
#[actix_web::get("/activity/{activity_id}/command")]
async fn get_running_command(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let agreement = get_activity_agreement(&db, &path.activity_id).await?;
    let msg = activity::GetRunningCommand {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    let cmd = agreement
        .provider_id()?
        .try_service(activity::BUS_ID)?
        .send(msg)
        .timeout(query.timeout)
        .await???;

    Ok::<_, Error>(web::Json(cmd))
}
