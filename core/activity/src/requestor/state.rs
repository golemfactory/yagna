use actix_web::{web, Responder};

use ya_client_model::market::Role;
use ya_core_model::activity;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{timeout::IntoTimeoutFuture, RpcEndpoint};

use crate::common::*;
use crate::error::Error;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope.service(get_running_command)
}

/// Get running command for a specified Activity.
#[actix_web::get("/activity/{activity_id}/command")]
async fn get_running_command(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id, Role::Requestor).await?;

    let agreement = get_activity_agreement(&db, &path.activity_id, Role::Requestor).await?;
    let msg = activity::GetRunningCommand {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    let cmd = agreement_provider_service(&id, &agreement)?
        .send(msg)
        .timeout(timeout_margin(query.timeout))
        .await???;

    Ok::<_, Error>(web::Json(cmd))
}
