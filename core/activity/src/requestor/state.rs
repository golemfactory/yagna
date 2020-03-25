use actix_web::{web, Responder};

use ya_core_model::activity;
use ya_model::activity::activity_state::StatePair;
use ya_model::activity::{ActivityState, ActivityUsage};
use ya_net::TryRemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{timeout::IntoTimeoutFuture, RpcEndpoint};

use crate::common::{
    authorize_activity_initiator, get_activity_agreement, PathActivity, QueryTimeout,
};
use crate::dao::{ActivityStateDao, ActivityUsageDao};
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

    // Return a locally persisted state if activity has been terminated
    let persisted_state = get_persisted_state(&db, &path.activity_id).await?;
    if persisted_state.terminated() {
        return Ok::<_, Error>(web::Json(persisted_state.unwrap()));
    }

    let agreement = get_activity_agreement(&db, &path.activity_id).await?;
    let msg = activity::GetState {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    // Retrieve and persist activity state
    let activity_state = agreement
        .provider_id()?
        .try_service(activity::BUS_ID)?
        .send(msg)
        .timeout(query.timeout)
        .await???;

    db.as_dao::<ActivityStateDao>()
        .set(
            &path.activity_id,
            activity_state.state.clone(),
            activity_state.reason.clone(),
            activity_state.error_message.clone(),
        )
        .await?;

    Ok::<_, Error>(web::Json(activity_state))
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

    // Return locally persisted usage if activity has been terminated
    let persisted_state = get_persisted_state(&db, &path.activity_id).await?;
    if persisted_state.terminated() {
        let persisted_usage = get_persisted_usage(&db, &path.activity_id).await?;
        if let Some(activity_usage) = persisted_usage {
            return Ok::<_, Error>(web::Json(activity_usage));
        }
    }

    let agreement = get_activity_agreement(&db, &path.activity_id).await?;
    let msg = activity::GetUsage {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    // Retrieve and persist activity usage
    let activity_usage = agreement
        .provider_id()?
        .try_service(activity::BUS_ID)?
        .send(msg)
        .timeout(query.timeout)
        .await???;

    db.as_dao::<ActivityUsageDao>()
        .set(&path.activity_id, &activity_usage.current_usage)
        .await?;

    Ok::<_, Error>(web::Json(activity_usage))
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

async fn get_persisted_state(
    db: &DbExecutor,
    activity_id: &str,
) -> Result<Option<ActivityState>, Error> {
    let maybe_state = db.as_dao::<ActivityStateDao>().get(activity_id).await?;

    if let Some(s) = maybe_state {
        let state: StatePair = serde_json::from_str(&s.name)?;
        if !state.alive() {
            return Ok(Some(ActivityState {
                state,
                reason: s.reason,
                error_message: s.error_message,
            }));
        }
    }

    Ok(None)
}

async fn get_persisted_usage(
    db: &DbExecutor,
    activity_id: &str,
) -> Result<Option<ActivityUsage>, Error> {
    let maybe_usage = db.as_dao::<ActivityUsageDao>().get(&activity_id).await?;

    if let Some(activity_usage) = maybe_usage {
        return Ok(Some(ActivityUsage {
            current_usage: activity_usage
                .vector_json
                .map(|json| serde_json::from_str(&json).unwrap()),
        }));
    }

    Ok(None)
}

trait TerminatedCheck {
    fn terminated(&self) -> bool;
}

impl TerminatedCheck for Option<ActivityState> {
    fn terminated(&self) -> bool {
        if let Some(s) = &self {
            return !s.state.alive();
        }
        false
    }
}
