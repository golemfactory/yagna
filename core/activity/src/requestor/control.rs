use actix_web::web;
use futures::prelude::*;
use serde::Deserialize;
use ya_core_model::activity::{CreateActivity, DestroyActivity, Exec, GetExecBatchResults};
use ya_core_model::ethaddr::NodeId;
use ya_core_model::market;
use ya_model::activity::activity_state::StatePair;
use ya_model::activity::{ExeScriptCommand, ExeScriptCommandResult, ExeScriptRequest, State};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

use crate::common::{
    generate_id, get_activity_agreement, is_activity_initiator, is_agreement_initiator,
    PathActivity, QueryTimeout, QueryTimeoutMaxCount,
};
use crate::dao::{ActivityDao, ActivityStateDao};
use crate::error::Error;
use crate::requestor::provider_activity_service_id;
use std::str::FromStr;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .route(
            "/activity",
            web::post().to(impl_restful_handler!(create_activity, query, body, id)),
        )
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
async fn create_activity(
    db: web::Data<DbExecutor>,
    query: web::Query<QueryTimeout>,
    body: web::Json<String>,
    id: Identity,
) -> Result<String, Error> {
    let agreement_id = body.into_inner();

    if !is_agreement_initiator(id.identity.to_string(), agreement_id.clone()).await? {
        return Err(Error::Forbidden.into());
    }

    let caller = Some(format!("/net/{}", id.name));
    log::debug!("caller from context: {:?}", caller);
    let agreement = bus::service(market::BUS_ID)
        .send(market::GetAgreement {
            agreement_id: agreement_id.clone(),
        })
        .await??;
    log::debug!("agreement: {:#?}", agreement);

    let msg = CreateActivity {
        // TODO: fix this
        provider_id: NodeId::from_str(agreement.offer.provider_id.as_ref().unwrap()).unwrap(),
        agreement_id: agreement_id.clone(),
        timeout: query.timeout.clone(),
    };

    let uri = provider_activity_service_id(&agreement)?;
    log::debug!("creating activity at: {}", uri);
    let activity_id = gsb_send!(caller, msg, &uri, query.timeout)?;
    log::debug!("creating activity: {}", activity_id);

    db.as_dao::<ActivityDao>()
        .create(&activity_id, &agreement_id)
        .await
        .map_err(Error::from)?;

    Ok(activity_id)
}

/// Destroys given Activity.
async fn destroy_activity(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> Result<(), Error> {
    if !is_activity_initiator(&db, id.identity.to_string(), &path.activity_id).await? {
        return Err(Error::Forbidden.into());
    }

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
    if !is_activity_initiator(&db, id.identity.to_string(), &path.activity_id).await? {
        return Err(Error::Forbidden.into());
    }

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
    Ok(batch_id)
}

/// Queries for ExeScript batch results.
async fn get_batch_results(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityBatch>,
    query: web::Query<QueryTimeoutMaxCount>,
    id: Identity,
) -> Result<Vec<ExeScriptCommandResult>, Error> {
    if !is_activity_initiator(&db, id.identity.to_string(), &path.activity_id).await? {
        return Err(Error::Forbidden.into());
    }

    let agreement = get_activity_agreement(&db, &path.activity_id, query.timeout.clone()).await?;
    let msg = GetExecBatchResults {
        activity_id: path.activity_id.to_string(),
        batch_id: path.batch_id.to_string(),
        timeout: query.timeout.clone(),
    };

    let uri = provider_activity_service_id(&agreement)?;
    gsb_send!(None, msg, &uri, query.timeout)
}

#[derive(Deserialize)]
struct PathActivityBatch {
    activity_id: String,
    batch_id: String,
}
