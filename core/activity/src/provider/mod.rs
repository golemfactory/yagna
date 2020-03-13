use actix_web::{web, Responder};
use futures::prelude::*;
use std::convert::From;

use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};
use ya_persistence::executor::DbExecutor;
use ya_persistence::models::ActivityEventType;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::timeout::IntoTimeoutFuture;

use crate::common::{PathActivity, QueryTimeoutMaxCount};
use crate::dao::*;
use crate::error::Error;

pub mod service;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .service(get_events_web)
        .service(get_activity_state_web)
        .service(set_activity_state_web)
        .service(get_activity_usage_web)
}

impl From<Event> for ProviderEvent {
    fn from(value: Event) -> Self {
        match value.event_type {
            ActivityEventType::CreateActivity => ProviderEvent::CreateActivity {
                activity_id: value.activity_natural_id,
                agreement_id: value.agreement_natural_id,
            },
            ActivityEventType::DestroyActivity => ProviderEvent::DestroyActivity {
                activity_id: value.activity_natural_id,
                agreement_id: value.agreement_natural_id,
            },
        }
    }
}

/// Get state of specified Activity.
async fn get_activity_state(
    db: &DbExecutor,
    activity_id: &str,
    identity_id: &str,
) -> Result<ActivityState, Error> {
    db.as_dao::<ActivityStateDao>()
        .get(activity_id, identity_id)
        .await
        .map_err(Error::from)?
        .map(|state| ActivityState {
            state: serde_json::from_str(&state.name).unwrap(),
            reason: state.reason,
            error_message: state.error_message,
        })
        .ok_or(Error::NotFound.into())
}

#[actix_web::get("/activity/{activity_id}/state")]
async fn get_activity_state_web(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    id: Identity,
) -> impl Responder {
    get_activity_state(&db, &path.activity_id, &id.identity.to_string())
        .await
        .map(web::Json)
}

/// Set state of specified Activity.
async fn set_activity_state(
    db: &DbExecutor,
    activity_id: &str,
    identity_id: &str,
    activity_state: ActivityState,
) -> Result<(), Error> {
    db.as_dao::<ActivityStateDao>()
        .set(
            activity_id,
            identity_id,
            activity_state.state.clone(),
            activity_state.reason.clone(),
            activity_state.error_message.clone(),
        )
        .await
        .map_err(|e| Error::from(e).into())
}

#[actix_web::put("/activity/{activity_id}/state")]
async fn set_activity_state_web(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    state: web::Json<ActivityState>,
    id: Identity,
) -> impl Responder {
    log::debug!("set_activity_state_web {:?}", state);
    set_activity_state(
        &db,
        &path.activity_id,
        &id.identity.to_string(),
        state.into_inner(),
    )
    .await
    .map(web::Json)
}

/// Get usage of specified Activity.
async fn get_activity_usage(
    db: &DbExecutor,
    activity_id: &str,
    identity_id: &str,
) -> Result<ActivityUsage, Error> {
    db.as_dao::<ActivityUsageDao>()
        .get(activity_id, identity_id)
        .await
        .map_err(Error::from)?
        .map(|usage| ActivityUsage {
            current_usage: usage
                .vector_json
                .map(|json| serde_json::from_str(&json).unwrap()),
        })
        .ok_or(Error::NotFound)
}

#[actix_web::get("/activity/{activity_id}/usage")]
async fn get_activity_usage_web(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    id: Identity,
) -> impl Responder {
    get_activity_usage(&db, &path.activity_id, &id.identity.to_string())
        .await
        .map(web::Json)
}

/// Fetch Requestor command events.
#[actix_web::get("/events")]
async fn get_events_web(
    db: web::Data<DbExecutor>,
    query: web::Query<QueryTimeoutMaxCount>,
    id: Identity,
) -> impl Responder {
    log::trace!("getting events {:?}", query);
    let events = db
        .as_dao::<EventDao>()
        .get_events_fut(&id.identity.to_string(), query.max_count)
        .timeout(query.timeout)
        .map_err(Error::from)
        .await??
        .into_iter()
        .map(ProviderEvent::from)
        .collect::<Vec<ProviderEvent>>();

    Result::<_, Error>::Ok(web::Json(events))
}
