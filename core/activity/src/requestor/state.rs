use crate::common::{PathActivity, QueryTimeout};
use crate::dao::{ActivityStateDao, ActivityUsageDao, NotFoundAsOption};
use crate::error::Error;
use crate::requestor::{get_agreement, missing_activity_err, provider_activity_uri};
use crate::ACTIVITY_API;
use actix_web::web;
use futures::lock::Mutex;
use futures::prelude::*;
use std::sync::Arc;
use ya_core_model::activity::{GetActivityState, GetActivityUsage, GetRunningCommand};
use ya_model::activity::{ActivityState, ActivityUsage, ExeScriptCommandState, State};
use ya_persistence::executor::DbExecutor;

pub fn web_scope(db: Arc<Mutex<DbExecutor>>) -> actix_web::Scope {
    let state = web::get().to(impl_restful_handler!(get_activity_state, path, query));
    let usage = web::get().to(impl_restful_handler!(get_activity_usage, path, query));
    let command = web::get().to(impl_restful_handler!(get_running_command, path, query));

    web::scope(&ACTIVITY_API)
        .data(db)
        .service(web::resource("/activity/{activity_id}/state").route(state))
        .service(web::resource("/activity/{activity_id}/usage").route(usage))
        .service(web::resource("/activity/{activity_id}/command").route(command))
}

/// Get state of specified Activity.
async fn get_activity_state(
    db: web::Data<Arc<Mutex<DbExecutor>>>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
) -> Result<ActivityState, Error> {
    let conn = db_conn!(db)?;
    missing_activity_err(&conn, &path.activity_id)?;

    let agreement = get_agreement(&conn, &path.activity_id)?;
    let uri = provider_activity_uri(&agreement.offer_node_id);
    let msg = GetActivityState {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    // Return a locally persisted state if activity has been terminated
    let dao = ActivityStateDao::new(&conn);
    let persisted_state = get_persisted_state(&dao, &path.activity_id)?;
    if persisted_state.terminated() {
        return Ok(persisted_state.unwrap());
    }

    // Retrieve and persist activity state
    let activity_state = gsb_send!(msg, &uri, query.timeout)?;
    dao.set(
        &path.activity_id,
        activity_state.state.clone(),
        activity_state.reason.clone(),
        activity_state.error_message.clone(),
    )?;

    Ok(activity_state)
}

/// Get usage of specified Activity.
async fn get_activity_usage(
    db: web::Data<Arc<Mutex<DbExecutor>>>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
) -> Result<ActivityUsage, Error> {
    let conn = db_conn!(db)?;
    missing_activity_err(&conn, &path.activity_id)?;

    let agreement = get_agreement(&conn, &path.activity_id)?;
    let uri = provider_activity_uri(&agreement.offer_node_id);
    let msg = GetActivityUsage {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    let state_dao = ActivityStateDao::new(&conn);
    let usage_dao = ActivityUsageDao::new(&conn);

    // Return locally persisted usage if activity has been terminated
    let persisted_state = get_persisted_state(&state_dao, &path.activity_id)?;
    if persisted_state.terminated() {
        let persisted_usage = get_persisted_usage(&usage_dao, &path.activity_id)?;
        if let Some(activity_usage) = persisted_usage {
            return Ok(activity_usage);
        }
    }

    // Retrieve and persist activity usage
    let activity_usage = gsb_send!(msg, &uri, query.timeout)?;
    usage_dao.set(&path.activity_id, &activity_usage.current_usage)?;

    Ok(activity_usage)
}

/// Get running command for a specified Activity.
async fn get_running_command(
    db: web::Data<Arc<Mutex<DbExecutor>>>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
) -> Result<ExeScriptCommandState, Error> {
    let conn = db_conn!(db)?;
    missing_activity_err(&conn, &path.activity_id)?;

    let agreement = get_agreement(&conn, &path.activity_id)?;
    let uri = provider_activity_uri(&agreement.offer_node_id);
    let msg = GetRunningCommand {
        activity_id: path.activity_id.to_string(),
        timeout: query.timeout.clone(),
    };

    gsb_send!(msg, &uri, query.timeout)
}

fn get_persisted_state(
    dao: &ActivityStateDao,
    activity_id: &str,
) -> Result<Option<ActivityState>, Error> {
    let maybe_state = dao
        .get(activity_id)
        .not_found_as_option()
        .map_err(Error::from)?;

    if let Some(s) = maybe_state {
        let state = serde_json::from_str(&s.name)?;
        if state == State::Terminated {
            return Ok(Some(ActivityState {
                state,
                reason: s.reason,
                error_message: s.error_message,
            }));
        }
    }

    Ok(None)
}

fn get_persisted_usage(
    dao: &ActivityUsageDao,
    activity_id: &str,
) -> Result<Option<ActivityUsage>, Error> {
    let maybe_usage = dao
        .get(&activity_id)
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
            return s.state == State::Terminated;
        }
        false
    }
}
