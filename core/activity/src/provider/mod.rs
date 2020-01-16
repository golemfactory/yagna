use crate::common::{PathActivity, QueryTimeoutMaxCount};
use crate::dao::*;
use crate::error::Error;
use crate::timeout::IntoTimeoutFuture;
use crate::ACTIVITY_API;
use actix_web::web;
use futures::lock::Mutex;
use futures::prelude::*;
use std::convert::From;
use std::sync::Arc;

use ya_model::activity::provider_event::ProviderEventType;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};
use ya_persistence::executor::DbExecutor;

pub mod service;

pub fn web_scope(db: Arc<Mutex<DbExecutor>>) -> actix_web::Scope {
    let events = web::get().to(impl_restful_handler!(get_events_web, query));
    let state = web::get().to(impl_restful_handler!(get_activity_state_web, path));
    let usage = web::get().to(impl_restful_handler!(get_activity_usage_web, path));

    web::scope(ACTIVITY_API)
        .data(db)
        .service(web::resource("/events").route(events))
        .service(web::resource("/activity/{activity_id}/state").route(state))
        .service(web::resource("/activity/{activity_id}/usage").route(usage))
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
async fn get_activity_state(
    db: &Arc<Mutex<DbExecutor>>,
    activity_id: &str,
) -> Result<ActivityState, Error> {
    ActivityStateDao::new(&db_conn!(db)?)
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
    db: web::Data<Arc<Mutex<DbExecutor>>>,
    path: web::Path<PathActivity>,
) -> Result<ActivityState, Error> {
    get_activity_state(&db, &path.activity_id).await
}

/// Get usage of specified Activity.
async fn get_activity_usage(
    db: &Arc<Mutex<DbExecutor>>,
    activity_id: &str,
) -> Result<ActivityUsage, Error> {
    ActivityUsageDao::new(&db_conn!(db)?)
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
    db: web::Data<Arc<Mutex<DbExecutor>>>,
    path: web::Path<PathActivity>,
) -> Result<ActivityUsage, Error> {
    get_activity_usage(&db, &path.activity_id).await
}

/// Fetch Requestor command events.
async fn get_events_web(
    db: web::Data<Arc<Mutex<DbExecutor>>>,
    query: web::Query<QueryTimeoutMaxCount>,
) -> Result<Vec<ProviderEvent>, Error> {
    EventDao::new(&db_conn!(db)?)
        .get_events_fut(query.max_count)
        .timeout(query.timeout)
        .map_err(Error::from)
        .await?
        .map_err(Error::from)
        .map(|events| events.into_iter().map(ProviderEvent::from).collect())
}
