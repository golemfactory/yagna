use crate::common::{PathActivity, QueryTimeout};
use crate::error::Error;
use crate::requestor::{get_agreement, uri};
use crate::ACTIVITY_SERVICE_URI;
use actix_web::web;
use futures::lock::Mutex;
use futures::prelude::*;
use std::sync::Arc;
use ya_core_model::activity::{GetActivityState, GetActivityUsage, GetRunningCommand};
use ya_model::activity::{ActivityState, ActivityUsage, ExeScriptCommandState};
use ya_persistence::executor::DbExecutor;

pub fn web_scope(db: Arc<Mutex<DbExecutor<Error>>>) -> actix_web::Scope {
    let state = web::get().to(impl_restful_handler!(get_activity_state, path, query));
    let usage = web::get().to(impl_restful_handler!(get_activity_usage, path, query));
    let command = web::get().to(impl_restful_handler!(get_running_command, path, query));

    web::scope(&ACTIVITY_SERVICE_URI)
        .data(db)
        .service(web::resource("/activity/{activity_id}/state").route(state))
        .service(web::resource("/activity/{activity_id}/usage").route(usage))
        .service(web::resource("/activity/{activity_id}/command").route(command))
}

/// Get state of specified Activity.
async fn get_activity_state(
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
) -> Result<ActivityState, Error> {
    let agreement = get_agreement(&db, &path.activity_id).await?;
    let uri = uri(&agreement.offer_node_id, "get_activity_state");
    let msg = GetActivityState {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    gsb_send!(msg, &uri, query.timeout)
}

/// Get usage of specified Activity.
async fn get_activity_usage(
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
) -> Result<ActivityUsage, Error> {
    let agreement = get_agreement(&db, &path.activity_id).await?;
    let uri = uri(&agreement.offer_node_id, "get_activity_usage");
    let msg = GetActivityUsage {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    gsb_send!(msg, &uri, query.timeout)
}

/// Get running command for a specified Activity.
async fn get_running_command(
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
) -> Result<ExeScriptCommandState, Error> {
    let agreement = get_agreement(&db, &path.activity_id).await?;
    let uri = uri(&agreement.offer_node_id, "get_running_command");
    let msg = GetRunningCommand {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    gsb_send!(msg, &uri, query.timeout)
}
