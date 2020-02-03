use crate::common::{
    fetch_agreement, generate_id, PathActivity, QueryTimeout, QueryTimeoutMaxCount,
};
use crate::dao::{ActivityDao, ActivityStateDao, AgreementDao};
use crate::error::Error;
use crate::requestor::{get_agreement, missing_activity_err, provider_activity_service_id};
use actix_web::web;
use futures::prelude::*;
use serde::Deserialize;
use std::convert::TryInto;
use ya_core_model::activity::{CreateActivity, DestroyActivity, Exec, GetExecBatchResults};
use ya_model::activity::{ExeScriptCommand, ExeScriptCommandResult, ExeScriptRequest, State};
use ya_persistence::executor::DbExecutor;
use ya_persistence::models::NewAgreement;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .route(
            "/activity",
            web::post().to(impl_restful_handler!(create_activity, path, body)),
        )
        .route(
            "/activity/{activity_id}",
            web::delete().to(impl_restful_handler!(destroy_activity, path, query)),
        )
        .route(
            "/activity/{activity_id}/exec",
            web::post().to(impl_restful_handler!(exec, path, query, body)),
        )
        .route(
            "/activity/{activity_id}/exec/{batch_id}",
            web::get().to(impl_restful_handler!(get_batch_results, path, query)),
        )
}

/// Creates new Activity based on given Agreement.
async fn create_activity(
    db: web::Data<DbExecutor>,
    query: web::Query<QueryTimeout>,
    body: web::Json<String>,
) -> Result<String, Error> {
    let conn = db_conn!(db)?;
    let agreement_id = body.into_inner();

    let agreement = fetch_agreement(&agreement_id).await?;
    log::info!("agreement: {:#?}", agreement);

    let new_agreement: NewAgreement = agreement.try_into()?;

    log::info!("inserting agreement: {:#?}", new_agreement);
    let uri = provider_activity_service_id(&new_agreement.offer_node_id);
    AgreementDao::new(&conn)
        .create(new_agreement)
        .map_err(Error::from)?;
    log::info!("agreement inserted");

    let msg = CreateActivity {
        agreement_id: agreement_id.clone(),
        timeout: query.timeout.clone(),
    };

    log::info!("creating activity at: {}", uri);
    let activity_id = gsb_send!(msg, &uri, query.timeout)?;
    log::info!("creating activity: {}", activity_id);

    ActivityDao::new(&conn)
        .create(&activity_id, &agreement_id)
        .map_err(Error::from)?;

    Ok(activity_id)
}

/// Destroys given Activity.
async fn destroy_activity(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
) -> Result<(), Error> {
    let conn = db_conn!(db)?;
    missing_activity_err(&conn, &path.activity_id)?;

    let agreement = get_agreement(&conn, &path.activity_id)?;
    let msg = DestroyActivity {
        activity_id: path.activity_id.to_string(),
        agreement_id: agreement.natural_id,
        timeout: query.timeout.clone(),
    };

    let uri = provider_activity_service_id(&agreement.offer_node_id);
    let _ = gsb_send!(msg, &uri, query.timeout)?;
    ActivityStateDao::new(&db_conn!(db)?)
        .set(&path.activity_id, State::Terminated, None, None)
        .map_err(Error::from)?;

    Ok(())
}

/// Executes an ExeScript batch within a given Activity.
async fn exec(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    body: web::Json<ExeScriptRequest>,
) -> Result<String, Error> {
    let conn = db_conn!(db)?;
    missing_activity_err(&conn, &path.activity_id)?;

    let commands: Vec<ExeScriptCommand> =
        serde_json::from_str(&body.text).map_err(|e| Error::BadRequest(format!("{:?}", e)))?;
    let agreement = get_agreement(&conn, &path.activity_id)?;
    let batch_id = generate_id();
    let msg = Exec {
        activity_id: path.activity_id.clone(),
        batch_id: batch_id.clone(),
        exe_script: commands,
        timeout: query.timeout.clone(),
    };

    let uri = provider_activity_service_id(&agreement.offer_node_id);
    gsb_send!(msg, &uri, query.timeout)?;
    Ok(batch_id)
}

/// Queries for ExeScript batch results.
async fn get_batch_results(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityBatch>,
    query: web::Query<QueryTimeoutMaxCount>,
) -> Result<Vec<ExeScriptCommandResult>, Error> {
    let conn = db_conn!(db)?;
    missing_activity_err(&conn, &path.activity_id)?;

    let agreement = get_agreement(&conn, &path.activity_id)?;
    let msg = GetExecBatchResults {
        activity_id: path.activity_id.to_string(),
        batch_id: path.batch_id.to_string(),
        timeout: query.timeout.clone(),
    };

    let uri = provider_activity_service_id(&agreement.offer_node_id);
    gsb_send!(msg, &uri, query.timeout)
}

#[derive(Deserialize)]
struct PathActivityBatch {
    activity_id: String,
    batch_id: String,
}
