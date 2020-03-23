use actix_web::{web, Responder};
use serde::Deserialize;
use std::str::FromStr;

use ya_core_model::{activity, ethaddr::NodeId};
use ya_model::activity::{activity_state::StatePair, ExeScriptCommand, ExeScriptRequest, State};
use ya_net::TryRemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{timeout::IntoTimeoutFuture, RpcEndpoint};

use crate::common::{
    authorize_activity_initiator, authorize_agreement_initiator, generate_id,
    get_activity_agreement, get_agreement, PathActivity, QueryTimeout, QueryTimeoutMaxCount,
};
use crate::dao::{ActivityDao, ActivityStateDao};
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

    let msg = activity::Create {
        provider_id: NodeId::from_str(agreement.provider_id()?)?,
        agreement_id: agreement_id.clone(),
        timeout: query.timeout.clone(),
    };

    let activity_id = agreement
        .provider_id()?
        .try_service(activity::BUS_ID)?
        .send(msg)
        .timeout(query.timeout)
        .await???;

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
    let msg = activity::Destroy {
        activity_id: path.activity_id.to_string(),
        agreement_id: agreement.agreement_id.clone(),
        timeout: query.timeout.clone(),
    };

    agreement
        .provider_id()?
        .try_service(activity::BUS_ID)?
        .send(msg)
        .timeout(query.timeout)
        .await???;

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
    let msg = activity::Exec {
        activity_id: path.activity_id.clone(),
        batch_id: batch_id.clone(),
        exe_script: commands,
        timeout: query.timeout.clone(),
    };

    agreement
        .provider_id()?
        .try_service(&activity::exeunit::bus_id(&path.activity_id))?
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
    query: web::Query<QueryTimeoutMaxCount>,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let agreement = get_activity_agreement(&db, &path.activity_id, query.timeout.clone()).await?;
    let msg = activity::GetExecBatchResults {
        activity_id: path.activity_id.to_string(),
        batch_id: path.batch_id.to_string(),
        timeout: query.timeout.clone(),
        // TODO: introduce field for query.max_count
    };

    let results = agreement
        .provider_id()?
        .try_service(&activity::exeunit::bus_id(&path.activity_id))?
        .send(msg)
        .timeout(query.timeout)
        .await???;

    Ok::<_, Error>(web::Json(results))
}

#[derive(Deserialize)]
struct PathActivityBatch {
    activity_id: String,
    batch_id: String,
}
