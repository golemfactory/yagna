use crate::common::{generate_id, PathActivity, QueryTimeoutMaxCount, RpcMessageResult};
use crate::dao::*;
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
    bind_gsb_method!(ACTIVITY_SERVICE_ID, db, create_activity_gsb);
    bind_gsb_method!(ACTIVITY_SERVICE_ID, db, destroy_activity_gsb);
    bind_gsb_method!(ACTIVITY_SERVICE_ID, db, get_activity_state_gsb);
    bind_gsb_method!(ACTIVITY_SERVICE_ID, db, set_activity_state_gsb);
    bind_gsb_method!(ACTIVITY_SERVICE_ID, db, get_activity_usage_gsb);
    bind_gsb_method!(ACTIVITY_SERVICE_ID, db, set_activity_usage_gsb);
}

pub fn web_scope(db: Arc<Mutex<DbExecutor<Error>>>) -> actix_web::Scope {
    let events = web::get().to(impl_restful_handler!(get_events_web, query));
    let state = web::get().to(impl_restful_handler!(get_activity_state_web, path));
    let usage = web::get().to(impl_restful_handler!(get_activity_usage_web, path));

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
async fn create_activity_gsb(
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
async fn destroy_activity_gsb(
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
    db: &Arc<Mutex<DbExecutor<Error>>>,
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

async fn get_activity_state_gsb(
    db: Arc<Mutex<DbExecutor<Error>>>,
    msg: GetActivityState,
) -> RpcMessageResult<GetActivityState> {
    get_activity_state(&db, &msg.activity_id)
        .await
        .map_err(Into::into)
}

async fn get_activity_state_web(
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    path: web::Path<PathActivity>,
) -> Result<ActivityState, Error> {
    get_activity_state(&db, &path.activity_id).await
}

/// Pass activity state (which may include error details).
async fn set_activity_state_gsb(
    db: Arc<Mutex<DbExecutor<Error>>>,
    msg: SetActivityState,
) -> RpcMessageResult<SetActivityState> {
    // TODO: caller authorization
    ActivityStateDao::new(&db_conn!(db)?)
        .set(
            &msg.activity_id,
            msg.state.state.clone(),
            msg.state.reason.clone(),
            msg.state.error_message.clone(),
        )
        .map_err(|e| Error::from(e).into())
}

/// Get usage of specified Activity.
async fn get_activity_usage(
    db: &Arc<Mutex<DbExecutor<Error>>>,
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

async fn get_activity_usage_gsb(
    db: Arc<Mutex<DbExecutor<Error>>>,
    msg: GetActivityUsage,
) -> RpcMessageResult<GetActivityUsage> {
    get_activity_usage(&db, &msg.activity_id)
        .await
        .map_err(Error::into)
}

async fn get_activity_usage_web(
    db: web::Data<Arc<Mutex<DbExecutor<Error>>>>,
    path: web::Path<PathActivity>,
) -> Result<ActivityUsage, Error> {
    get_activity_usage(&db, &path.activity_id).await
}

/// Pass current activity usage (which may include error details).
async fn set_activity_usage_gsb(
    db: Arc<Mutex<DbExecutor<Error>>>,
    msg: SetActivityUsage,
) -> RpcMessageResult<SetActivityUsage> {
    // TODO: caller authorization
    ActivityUsageDao::new(&db_conn!(db)?)
        .set(&msg.activity_id, &msg.usage.current_usage)
        .map_err(|e| Error::from(e).into())
}

/// Fetch Requestor command events.
async fn get_events_web(
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
