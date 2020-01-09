use crate::common::{generate_id, PathActivity, QueryTimeout, QueryTimeoutMaxCount};
use crate::dao::AgreementDao;
use crate::error::Error;
use crate::requestor::{get_agreement, uri};
use crate::ACTIVITY_SERVICE_URI;
use actix_web::web;
use futures::lock::Mutex;
use futures::prelude::*;
use serde::Deserialize;
use std::sync::Arc;
use ya_core_model::activity::{CreateActivity, DestroyActivity, Exec, GetExecBatchResults};
use ya_model::activity::{ExeScriptCommand, ExeScriptCommandResult, ExeScriptRequest};
use ya_persistence::executor::DbExecutor;

pub fn web_scope(db: Arc<Mutex<DbExecutor<Error>>>) -> actix_web::Scope {
    let create = web::post().to(impl_restful_handler!(create_activity, path, query));
    let delete = web::delete().to(impl_restful_handler!(destroy_activity, path, query));
    let exec = web::post().to(impl_restful_handler!(exec, path, query, body));
    let batch = web::get().to(impl_restful_handler!(get_batch_results, path, query));

    web::scope(&ACTIVITY_SERVICE_URI)
        .data(db)
        .service(web::resource("/activity").route(create))
        .service(web::resource("/activity/{activity_id}").route(delete))
        .service(web::resource("/activity/{activity_id}/exec").route(exec))
        .service(web::resource("/activity/{activity_id}/exec/{batch_id}").route(batch))
}

/// Creates new Activity based on given Agreement.
async fn create_activity(
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    query: web::Query<QueryTimeout>,
    body: web::Json<CreateActivity>,
) -> Result<String, Error> {
    let agreement = AgreementDao::new(&db_conn!(db)?).get(&body.agreement_id)?;
    let uri = uri(&agreement.offer_node_id, "create_activity");

    gsb_send!(body.into_inner(), &uri, query.timeout)
}

/// Destroys given Activity.
async fn destroy_activity(
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
) -> Result<(), Error> {
    let agreement = get_agreement(&db, &path.activity_id).await?;
    let uri = uri(&agreement.offer_node_id, "destroy_activity");
    let msg = DestroyActivity {
        activity_id: path.activity_id.to_string(),
        agreement_id: agreement.natural_id,
        timeout: query.timeout.clone(),
    };

    gsb_send!(msg, &uri, query.timeout)
}

/// Executes an ExeScript batch within a given Activity.
async fn exec(
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    body: web::Json<ExeScriptRequest>,
) -> Result<String, Error> {
    let commands: Vec<ExeScriptCommand> =
        serde_json::from_str(&body.text).map_err(|e| Error::BadRequest(format!("{:?}", e)))?;
    let agreement = get_agreement(&db, &path.activity_id).await?;
    let uri = uri(&agreement.offer_node_id, "destroy_activity");
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
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    path: web::Path<PathActivityBatch>,
    query: web::Query<QueryTimeoutMaxCount>,
) -> Result<Vec<ExeScriptCommandResult>, Error> {
    let agreement = get_agreement(&db, &path.activity_id).await?;
    let uri = uri(&agreement.offer_node_id, "get_exec_batch_results");
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
