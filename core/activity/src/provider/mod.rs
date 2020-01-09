use crate::common::{generate_id, PathActivity, QueryTimeoutMaxCount, RpcMessageResult};
use crate::dao::{
    ActivityDao, ActivityStateDao, ActivityUsageDao, AgreementDao, Event, EventDao,
    NotFoundAsOption,
};
use crate::error::Error;
use crate::timeout::IntoTimeoutFuture;
use crate::{ACTIVITY_SERVICE_ID, ACTIVITY_SERVICE_URI};
use actix_web::web;
use futures::lock::Mutex;
use futures::prelude::*;
use std::convert::From;
use std::sync::Arc;
use ya_core_model::activity::*;
use ya_model::activity::provider_event::ProviderEventType;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent, State};
use ya_persistence::executor::DbExecutor;

pub fn bind_gsb(db: Arc<Mutex<DbExecutor<Error>>>) {
    bind_gsb_method!(ACTIVITY_SERVICE_ID, db, create_activity);
    bind_gsb_method!(ACTIVITY_SERVICE_ID, db, destroy_activity);
    bind_gsb_method!(ACTIVITY_SERVICE_ID, db, get_activity_state);
    bind_gsb_method!(ACTIVITY_SERVICE_ID, db, get_activity_usage);
}

pub fn web_scope(db: Arc<Mutex<DbExecutor<Error>>>) -> actix_web::Scope {
    let events = web::get().to(impl_restful_handler!(get_events, query));
    let state = web::put().to(impl_restful_handler!(set_activity_state, path, body));
    let usage = web::put().to(impl_restful_handler!(set_activity_usage, path, body));

    web::scope(&ACTIVITY_SERVICE_URI)
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

/// Creates new Activity based on given Agreement.
async fn create_activity(
    db: Arc<Mutex<DbExecutor<Error>>>,
    msg: CreateActivity,
) -> RpcMessageResult<CreateActivity> {
    let conn = db_conn!(db)?;
    let activity_id = generate_id();

    // Check whether agreement exists
    AgreementDao::new(&conn)
        .get(&msg.agreement_id)
        .map_err(Error::from)?;

    ActivityDao::new(&conn)
        .create(&activity_id, &msg.agreement_id)
        .map_err(Error::from)?;

    EventDao::new(&conn)
        .create(
            &activity_id,
            serde_json::to_string(&ProviderEventType::CreateActivity)
                .unwrap()
                .as_str(),
        )
        .map_err(Error::from)?;

    ActivityStateDao::new(&conn)
        .get_future(&activity_id, None)
        .timeout(msg.timeout)
        .map_err(Error::from)
        .await
        .map_err(Error::from)?;

    Ok(activity_id)
}

/// Destroys given Activity.
async fn destroy_activity(
    db: Arc<Mutex<DbExecutor<Error>>>,
    msg: DestroyActivity,
) -> RpcMessageResult<DestroyActivity> {
    let conn = db_conn!(db)?;

    EventDao::new(&conn)
        .create(
            &msg.activity_id,
            serde_json::to_string(&ProviderEventType::DestroyActivity)
                .unwrap()
                .as_str(),
        )
        .map_err(Error::from)?;

    ActivityStateDao::new(&conn)
        .get_future(&msg.activity_id, Some(State::Terminated))
        .timeout(msg.timeout)
        .map_err(Error::from)
        .await?;

    Ok(())
}

/// Get state of specified Activity.
async fn get_activity_state(
    db: Arc<Mutex<DbExecutor<Error>>>,
    msg: GetActivityState,
) -> RpcMessageResult<GetActivityState> {
    ActivityStateDao::new(&db_conn!(db)?)
        .get(&msg.activity_id)
        .not_found_as_option()
        .map_err(Error::from)?
        .map(|state| ActivityState {
            state: serde_json::from_str(&state.name).unwrap(),
            reason: state.reason,
            error_message: state.error_message,
        })
        .ok_or(Error::NotFound.into())
}

/// Pass activity state (which may include error details).
async fn set_activity_state(
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    path: web::Path<PathActivity>,
    activity_state: web::Json<ActivityState>,
) -> Result<(), Error> {
    ActivityStateDao::new(&db_conn!(db)?)
        .set(
            &path.activity_id,
            activity_state.state.clone(),
            activity_state.reason.clone(),
            activity_state.error_message.clone(),
        )
        .map_err(Error::from)
}

/// Get usage of specified Activity.
async fn get_activity_usage(
    db: Arc<Mutex<DbExecutor<Error>>>,
    msg: GetActivityUsage,
) -> RpcMessageResult<GetActivityUsage> {
    ActivityUsageDao::new(&db_conn!(db)?)
        .get(&msg.activity_id)
        .not_found_as_option()
        .map_err(Error::from)?
        .map(|usage| ActivityUsage {
            current_usage: usage
                .vector_json
                .map(|json| serde_json::from_str(&json).unwrap()),
        })
        .ok_or(Error::NotFound.into())
}

/// Pass current activity usage (which may include error details).
async fn set_activity_usage(
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    path: web::Path<PathActivity>,
    activity_usage: web::Json<ActivityUsage>,
) -> Result<(), Error> {
    ActivityUsageDao::new(&db_conn!(db)?)
        .set(&path.activity_id, &activity_usage.current_usage)
        .map_err(Error::from)
}

/// Fetch Requestor command events.
async fn get_events(
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    query: web::Query<QueryTimeoutMaxCount>,
) -> Result<Vec<ProviderEvent>, Error> {
    EventDao::new(&db_conn!(db)?)
        .get_events_fut(query.max_count)
        .timeout(query.timeout)
        .map_err(Error::from)
        .await
        .map_err(Error::from)
        .map(|events| events.into_iter().map(ProviderEvent::from).collect())
}
