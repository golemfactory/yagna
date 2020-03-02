use actix_web::web;
use futures::prelude::*;

use ya_core_model::activity::{GetActivityState, GetActivityUsage, GetRunningCommand};
use ya_model::activity::{ActivityState, ActivityUsage, ExeScriptCommandState};
use ya_persistence::executor::DbExecutor;

use crate::common::{
    authorize_activity_initiator, get_activity_agreement, PathActivity, QueryTimeout,
};
use crate::dao::{ActivityStateDao, ActivityUsageDao, NotFoundAsOption};
use crate::error::Error;
use crate::requestor::provider_activity_service_id;
use ya_model::activity::activity_state::StatePair;
use ya_service_api_web::middleware::Identity;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .route(
            "/activity/{activity_id}/state",
            web::get().to(impl_restful_handler!(get_activity_state, path, query, id)),
        )
        .route(
            "/activity/{activity_id}/usage",
            web::get().to(impl_restful_handler!(get_activity_usage, path, query, id)),
        )
        .route(
            "/activity/{activity_id}/command",
            web::get().to(impl_restful_handler!(get_running_command, path, query, id)),
        )
}

/// Get state of specified Activity.
async fn get_activity_state(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> Result<ActivityState, Error> {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let agreement = get_activity_agreement(&db, &path.activity_id, query.timeout.clone()).await?;
    let msg = GetActivityState {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    // Return a locally persisted state if activity has been terminated
    let persisted_state = get_persisted_state(&db, &path.activity_id).await?;
    if persisted_state.terminated() {
        return Ok(persisted_state.unwrap());
    }

    // Retrieve and persist activity state
    let uri = provider_activity_service_id(&agreement)?;
    let activity_state = gsb_send!(None, msg, &uri, query.timeout)?;
    db.as_dao::<ActivityStateDao>()
        .set(
            &path.activity_id,
            activity_state.state.clone(),
            activity_state.reason.clone(),
            activity_state.error_message.clone(),
        )
        .await?;

    Ok(activity_state)
}

/// Get usage of specified Activity.
async fn get_activity_usage(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> Result<ActivityUsage, Error> {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let agreement = get_activity_agreement(&db, &path.activity_id, query.timeout.clone()).await?;
    let msg = GetActivityUsage {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    // Return locally persisted usage if activity has been terminated
    let persisted_state = get_persisted_state(&db, &path.activity_id).await?;
    if persisted_state.terminated() {
        let persisted_usage = get_persisted_usage(&db, &path.activity_id).await?;
        if let Some(activity_usage) = persisted_usage {
            return Ok(activity_usage);
        }
    }

    // Retrieve and persist activity usage
    let uri = provider_activity_service_id(&agreement)?;
    let activity_usage = gsb_send!(None, msg, &uri, query.timeout)?;
    db.as_dao::<ActivityUsageDao>()
        .set(&path.activity_id, &activity_usage.current_usage)
        .await?;

    Ok(activity_usage)
}

/// Get running command for a specified Activity.
async fn get_running_command(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> Result<ExeScriptCommandState, Error> {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let agreement = get_activity_agreement(&db, &path.activity_id, query.timeout.clone()).await?;
    let msg = GetRunningCommand {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    let uri = provider_activity_service_id(&agreement)?;
    gsb_send!(None, msg, &uri, query.timeout)
}

async fn get_persisted_state(
    db: &DbExecutor,
    activity_id: &str,
) -> Result<Option<ActivityState>, Error> {
    let maybe_state = db
        .as_dao::<ActivityStateDao>()
        .get(activity_id)
        .await
        .not_found_as_option()
        .map_err(Error::from)?;

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
    let maybe_usage = db
        .as_dao::<ActivityUsageDao>()
        .get(&activity_id)
        .await
        .not_found_as_option()
        .map_err(Error::from)?;

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
