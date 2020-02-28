use actix_web::{web, Responder};
use futures::prelude::*;
use serde::Deserialize;
use std::str::FromStr;

use ya_core_model::{
    activity::{CreateActivity, DestroyActivity, Exec, GetExecBatchResults},
    ethaddr::NodeId,
};
use ya_model::activity::activity_state::StatePair;
use ya_model::activity::{ExeScriptCommand, ExeScriptCommandResult, ExeScriptRequest, State};
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
        .route(
            "/activity/{activity_id}",
            web::delete().to(impl_restful_handler!(destroy_activity, path, query, id)),
        )
        .route(
            "/activity/{activity_id}/exec",
            web::post().to(impl_restful_handler!(exec, path, query, body, id)),
        )
        .route(
            "/activity/{activity_id}/exec/{batch_id}",
            web::get().to(impl_restful_handler!(get_batch_results, path, query, id)),
        )
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
    authorize_agreement_initiator(id.identity, agreement_id.clone()).await?;

    let agreement = get_agreement(&agreement_id).await?;
    log::trace!("agreement: {:#?}", agreement);

    let msg = CreateActivity {
        // TODO: fix this
        provider_id: NodeId::from_str(agreement.offer.provider_id.as_ref().unwrap()).unwrap(),
        agreement_id: agreement_id.clone(),
        timeout_ms: query.timeout_ms.clone(),
    };

    let caller = Some(format!("/net/{:?}", id.identity));
    let uri = provider_activity_service_id(&agreement)?;

    log::debug!("creating activity at: {}, caller: {:?}", uri, caller);
    let activity_id = gsb_send!(caller, msg, &uri, query.timeout_ms)?;

    log::debug!("activity created: {}, inserting", activity_id);
    db.as_dao::<ActivityDao>()
        .create(&activity_id, &agreement_id)
        .await?;

    Ok::<_, Error>(web::Json(activity_id))
}

/// Destroys given Activity.
async fn destroy_activity(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> Result<(), Error> {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let agreement =
        get_activity_agreement(&db, &path.activity_id, query.timeout_ms.clone()).await?;
    let msg = DestroyActivity {
        activity_id: path.activity_id.to_string(),
        agreement_id: agreement.agreement_id.clone(),
        timeout_ms: query.timeout_ms.clone(),
    };

    let uri = provider_activity_service_id(&agreement)?;
    let _ = gsb_send!(None, msg, &uri, query.timeout_ms)?;
    db.as_dao::<ActivityStateDao>()
        .set(
            &path.activity_id,
            StatePair(State::Terminated, None),
            None,
            None,
        )
        .await
        .map_err(Error::from)?;

    Ok(())
}

/// Executes an ExeScript batch within a given Activity.
async fn exec(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    body: web::Json<ExeScriptRequest>,
    id: Identity,
) -> Result<String, Error> {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let commands: Vec<ExeScriptCommand> =
        serde_json::from_str(&body.text).map_err(|e| Error::BadRequest(format!("{:?}", e)))?;
    let agreement =
        get_activity_agreement(&db, &path.activity_id, query.timeout_ms.clone()).await?;
    let batch_id = generate_id();
    let msg = Exec {
        activity_id: path.activity_id.clone(),
        batch_id: batch_id.clone(),
        exe_script: commands,
        timeout_ms: query.timeout_ms.clone(),
    };

    let uri = provider_activity_service_id(&agreement)?;
    gsb_send!(None, msg, &uri, query.timeout_ms)?;
    Ok(batch_id)
}

/// Queries for ExeScript batch results.
async fn get_batch_results(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityBatch>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> Result<Vec<ExeScriptCommandResult>, Error> {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let agreement =
        get_activity_agreement(&db, &path.activity_id, query.timeout_ms.clone()).await?;
    let msg = GetExecBatchResults {
        activity_id: path.activity_id.to_string(),
        batch_id: path.batch_id.to_string(),
        timeout_ms: query.timeout_ms.clone(),
    };

    let uri = provider_activity_service_id(&agreement)?;
    gsb_send!(None, msg, &uri, query.timeout_ms)
}

#[derive(Deserialize)]
struct PathActivityBatch {
    activity_id: String,
    batch_id: String,
}
