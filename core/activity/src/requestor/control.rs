use actix_web::{web, Responder};
use futures::prelude::*;
use serde::Deserialize;
use std::str::FromStr;

use ya_core_model::{
    activity::{CreateActivity, DestroyActivity, Exec, GetExecBatchResults},
    ethaddr::NodeId,
};
use ya_model::activity::activity_state::StatePair;
use ya_model::activity::{ExeScriptCommand, ExeScriptRequest, State};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::common::{
    authorize_activity_initiator, authorize_agreement_initiator, generate_id,
    get_activity_agreement, get_agreement, PathActivity, QueryTimeout,
};
use crate::dao::{ActivityDao, ActivityStateDao};
use crate::error::Error;
use crate::requestor::provider_activity_service_id;

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

    // Note: empty string will be invalid id in NodeId::from_str function.
    // FIXME: Should provider_id be optional? Or we can take this id from somewhere else?
    let node_id = agreement.offer.provider_id.clone().unwrap_or("".to_string());
    let provider_id = NodeId::from_str(&node_id)
        .map_err(|error| Error::Service(format!("Invalid node id [{}]: {}", &node_id, error)))?;

    let msg = CreateActivity {
        provider_id,
        agreement_id: agreement_id.clone(),
        timeout: query.timeout.clone(),
    };

    let caller = Some(format!("/net/{:?}", id.identity));
    let uri = provider_activity_service_id(&agreement)?;

    log::debug!("creating activity at: {}, caller: {:?}", uri, caller);
    let activity_id = gsb_send!(caller, msg, &uri, query.timeout)?;

    log::debug!("activity created: {}, inserting", activity_id);
    db.as_dao::<ActivityDao>()
        .create_if_not_exists(&activity_id, &agreement_id)
        .await
        .map_err(Error::from)?;

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

    let agreement = get_activity_agreement(&db, &path.activity_id, query.timeout.clone()).await?;
    let msg = DestroyActivity {
        activity_id: path.activity_id.to_string(),
        agreement_id: agreement.agreement_id.clone(),
        timeout: query.timeout.clone(),
    };

    let uri = provider_activity_service_id(&agreement)?;
    let _ = gsb_send!(None, msg, &uri, query.timeout)?;
    db.as_dao::<ActivityStateDao>()
        .set(
            &path.activity_id,
            StatePair(State::Terminated, None),
            None,
            None,
        )
        .await
        .map_err(Error::from)?;

    Ok::<_, Error>(web::Json(()))
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
    let agreement = get_activity_agreement(&db, &path.activity_id, query.timeout.clone()).await?;
    let batch_id = generate_id();
    let msg = Exec {
        activity_id: path.activity_id.clone(),
        batch_id: batch_id.clone(),
        exe_script: commands,
        timeout: query.timeout.clone(),
    };

    let uri = provider_activity_service_id(&agreement)?;
    gsb_send!(None, msg, &uri, query.timeout)?;

    Ok::<_, Error>(web::Json(batch_id))
}

/// Queries for ExeScript batch results.
#[actix_web::get("/activity/{activity_id}/exec/{batch_id}")]
async fn get_batch_results(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityBatch>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let agreement = get_activity_agreement(&db, &path.activity_id, query.timeout.clone()).await?;
    let msg = GetExecBatchResults {
        activity_id: path.activity_id.to_string(),
        batch_id: path.batch_id.to_string(),
        timeout: query.timeout.clone(),
    };

    let uri = provider_activity_service_id(&agreement)?;
    let results = gsb_send!(None, msg, &uri, query.timeout)?;

    Ok::<_, Error>(web::Json(results))
}

#[derive(Deserialize)]
struct PathActivityBatch {
    activity_id: String,
    batch_id: String,
}
