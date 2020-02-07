use crate::common::{is_activity_executor, PathActivity, QueryTimeoutMaxCount};
use crate::dao::*;
use crate::error::Error;
use crate::{db_conn, impl_restful_handler};
use actix_web::web;
use futures::prelude::*;
use std::convert::From;

use ya_model::activity::provider_event::ProviderEventType;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};
use ya_persistence::executor::{ConnType, DbExecutor};
use ya_service_api_web::middleware::Identity;
use ya_service_bus::timeout::IntoTimeoutFuture;

pub mod service;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .route(
            "/events",
            web::get().to(impl_restful_handler!(get_events_web, query)),
        )
        .route(
            "/activity/{activity_id}/state",
            web::get().to(impl_restful_handler!(get_activity_state_web, path, id)),
        )
        .route(
            "/activity/{activity_id}/state",
            web::put().to(impl_restful_handler!(
                set_activity_state_web,
                path,
                state,
                id
            )),
        )
        .route(
            "/activity/{activity_id}/usage",
            web::get().to(impl_restful_handler!(get_activity_usage_web, path, id)),
        )
}

impl From<Event> for ProviderEvent {
    fn from(value: Event) -> Self {
        let event_type = serde_json::from_str::<ProviderEventType>(&value.name).unwrap();
        match event_type {
            ProviderEventType::CreateActivity => ProviderEvent::CreateActivity {
                activity_id: value.activity_natural_id,
                agreement_id: value.agreement_natural_id,
            },
            ProviderEventType::DestroyActivity => ProviderEvent::DestroyActivity {
                activity_id: value.activity_natural_id,
                agreement_id: value.agreement_natural_id,
            },
        }
    }
}

/// Get state of specified Activity.
async fn get_activity_state(conn: &ConnType, activity_id: &str) -> Result<ActivityState, Error> {
    ActivityStateDao::new(conn)
        .get(activity_id)
        .not_found_as_option()
        .map_err(Error::from)?
        .map(|state| ActivityState {
            state: serde_json::from_str(&state.name).unwrap(),
            reason: state.reason,
            error_message: state.error_message,
        })
        .ok_or(Error::NotFound.into())
}

async fn get_activity_state_web(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    id: Identity,
) -> Result<ActivityState, Error> {
    let conn = &db_conn!(db)?;
    if !is_activity_executor(&conn, id.name, &path.activity_id).await? {
        return Err(Error::Forbidden.into());
    }

    get_activity_state(&conn, &path.activity_id).await
}

/// Set state of specified Activity.
async fn set_activity_state(
    conn: &ConnType,
    activity_id: &str,
    activity_state: ActivityState,
) -> Result<(), Error> {
    ActivityStateDao::new(&conn)
        .set(
            &activity_id,
            activity_state.state.clone(),
            activity_state.reason.clone(),
            activity_state.error_message.clone(),
        )
        .map_err(|e| Error::from(e).into())
}

async fn set_activity_state_web(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    state: web::Json<ActivityState>,
    id: Identity,
) -> Result<(), Error> {
    let conn = &db_conn!(db)?;
    if !is_activity_executor(&conn, id.name, &path.activity_id).await? {
        return Err(Error::Forbidden.into());
    }

    set_activity_state(&conn, &path.activity_id, state.into_inner()).await
}

/// Get usage of specified Activity.
async fn get_activity_usage(conn: &ConnType, activity_id: &str) -> Result<ActivityUsage, Error> {
    ActivityUsageDao::new(conn)
        .get(activity_id)
        .not_found_as_option()
        .map_err(Error::from)?
        .map(|usage| ActivityUsage {
            current_usage: usage
                .vector_json
                .map(|json| serde_json::from_str(&json).unwrap()),
        })
        .ok_or(Error::NotFound)
}

async fn get_activity_usage_web(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    id: Identity,
) -> Result<ActivityUsage, Error> {
    let conn = &db_conn!(db)?;
    if !is_activity_executor(&conn, id.name, &path.activity_id).await? {
        return Err(Error::Forbidden.into());
    }

    get_activity_usage(&conn, &path.activity_id).await
}

/// Fetch Requestor command events.
async fn get_events_web(
    db: web::Data<DbExecutor>,
    query: web::Query<QueryTimeoutMaxCount>,
) -> Result<Vec<ProviderEvent>, Error> {
    log::debug!("getting events");

    EventDao::new(&db_conn!(db)?)
        .get_events_fut(query.max_count)
        .timeout(query.timeout)
        .map_err(Error::from)
        .await?
        .map_err(Error::from)
        .map(|events| events.into_iter().map(ProviderEvent::from).collect())
}
