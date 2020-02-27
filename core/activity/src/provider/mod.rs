use actix_web::{web, Responder};
use futures::prelude::*;
use std::convert::From;

use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};
use ya_persistence::executor::DbExecutor;
use ya_persistence::models::ActivityEventType;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::timeout::IntoTimeoutFuture;

use crate::common::{authorize_activity_executor, PathActivity, QueryTimeoutMaxCount};
use crate::dao::*;
use crate::error::Error;
use crate::impl_restful_handler;

pub mod service;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .route(
            "/events",
            web::get().to(impl_restful_handler!(get_events_web, query)),
        )
        .service(get_activity_state_web)
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
async fn get_activity_state(db: &DbExecutor, activity_id: &str) -> Result<ActivityState, Error> {
    db.as_dao::<ActivityStateDao>()
        .get(activity_id)
        .await
        .not_found_as_option()
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
    authorize_activity_executor(&db, id.identity, &path.activity_id).await?;

    get_activity_state(&db, &path.activity_id)
        .await
        .map(web::Json)
}

/// Set state of specified Activity.
async fn set_activity_state(
    db: &DbExecutor,
    activity_id: &str,
    activity_state: ActivityState,
) -> Result<(), Error> {
    db.as_dao::<ActivityStateDao>()
        .set(
            &activity_id,
            activity_state.state.clone(),
            activity_state.reason.clone(),
            activity_state.error_message.clone(),
        )
        .await
        .map_err(|e| Error::from(e).into())
}

async fn set_activity_state_web(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    state: web::Json<ActivityState>,
    id: Identity,
) -> Result<(), Error> {
    authorize_activity_executor(&db, id.identity, &path.activity_id).await?;

    set_activity_state(&db, &path.activity_id, state.into_inner()).await
}

/// Get usage of specified Activity.
async fn get_activity_usage(db: &DbExecutor, activity_id: &str) -> Result<ActivityUsage, Error> {
    db.as_dao::<ActivityUsageDao>()
        .get(activity_id)
        .await
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
    authorize_activity_executor(&db, id.identity, &path.activity_id).await?;

    get_activity_usage(&db, &path.activity_id).await
}

/// Fetch Requestor command events.
async fn get_events_web(
    db: web::Data<DbExecutor>,
    query: web::Query<QueryTimeoutMaxCount>,
) -> Result<Vec<ProviderEvent>, Error> {
    log::debug!("getting events");

    Ok(db
        .as_dao::<EventDao>()
        .get_events_fut(query.max_count)
        .timeout(query.timeout)
        .map_err(Error::from)
        .await??
        .into_iter()
        .map(ProviderEvent::from)
        .collect())
}
