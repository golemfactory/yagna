use crate::common::{generate_id, PathActivity, QueryTimeout, QueryTimeoutMaxCount};
use crate::dao::{ActivityDao, ActivityStateDao, AgreementDao, NotFoundAsOption};
use crate::error::Error;
use crate::requestor::{get_agreement, missing_activity_err, provider_activity_uri};
use actix_web::web;
use futures::prelude::*;
use serde::Deserialize;
use ya_core_model::activity::{CreateActivity, DestroyActivity, Exec, GetExecBatchResults};
use ya_model::activity::{ExeScriptCommand, ExeScriptCommandResult, ExeScriptRequest, State};
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::service;
use ya_service_bus::RpcEndpoint;

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
    log::debug!("getting agrement from DB");
    let agreement = AgreementDao::new(&conn)
        .get(&agreement_id)
        .not_found_as_option()?
        .ok_or(Error::BadRequest(format!(
            "Unknown agreement id: {}",
            agreement_id
        )))?;
    log::debug!("agreement: {:#?}", agreement);

    let uri = provider_activity_uri(&agreement.offer_node_id);

    // TODO: remove /private from /net calls !!
    let activity_id = service(&format!("/private{}", uri))
        .send(CreateActivity {
            agreement_id: agreement_id.clone(),
            // TODO: Add timeout from parameter
            timeout: Some(600),
        })
        .await??;
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
    let uri = provider_activity_uri(&agreement.offer_node_id);
    let msg = DestroyActivity {
        activity_id: path.activity_id.to_string(),
        agreement_id: agreement.natural_id,
        timeout: query.timeout.clone(),
    };

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
    let uri = provider_activity_uri(&agreement.offer_node_id);
    let batch_id = generate_id();
    let msg = Exec {
        activity_id: path.activity_id.clone(),
        batch_id: batch_id.clone(),
        exe_script: commands,
        timeout: query.timeout.clone(),
    };

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
    let uri = provider_activity_uri(&agreement.offer_node_id);
    let msg = GetExecBatchResults {
        activity_id: path.activity_id.to_string(),
        batch_id: path.batch_id.to_string(),
        timeout: query.timeout.clone(),
    };

    gsb_send!(msg, &uri, query.timeout)
}

#[derive(Deserialize)]
struct PathActivityBatch {
    activity_id: String,
    batch_id: String,
}
