use crate::common::{generate_id, RpcMessageResult};
use crate::dao::*;
use crate::error::Error;
use crate::timeout::IntoTimeoutFuture;

use futures::lock::Mutex;
use futures::prelude::*;
use std::convert::From;
use std::sync::Arc;
use ya_core_model::activity::*;
use ya_model::activity::provider_event::ProviderEventType;
use ya_model::activity::State;
use ya_persistence::executor::DbExecutor;

pub fn bind_gsb(db: Arc<Mutex<DbExecutor>>) {
    log::info!("activating activity provider service");

    // public for remote requestors interactions
    bind_gsb_method!(bind_public, ACTIVITY_SERVICE_ID, db, create_activity_gsb);
    bind_gsb_method!(bind_public, ACTIVITY_SERVICE_ID, db, destroy_activity_gsb);
    bind_gsb_method!(bind_public, ACTIVITY_SERVICE_ID, db, get_activity_state_gsb);
    bind_gsb_method!(bind_public, ACTIVITY_SERVICE_ID, db, get_activity_usage_gsb);

    // local for ExeUnit interactions
    bind_gsb_method!(
        bind_private,
        ACTIVITY_SERVICE_ID,
        db,
        set_activity_state_gsb
    );
    bind_gsb_method!(
        bind_private,
        ACTIVITY_SERVICE_ID,
        db,
        set_activity_usage_gsb
    );

    log::info!("activity provider service activated");
}

/// Creates new Activity based on given Agreement.
async fn create_activity_gsb(
    db: Arc<Mutex<DbExecutor>>,
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
        .await?
        .map_err(Error::from)?;

    Ok(activity_id)
}

/// Destroys given Activity.
async fn destroy_activity_gsb(
    db: Arc<Mutex<DbExecutor>>,
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
        .await?
        .map_err(Error::from)?;

    Ok(())
}

async fn get_activity_state_gsb(
    db: Arc<Mutex<DbExecutor>>,
    msg: GetActivityState,
) -> RpcMessageResult<GetActivityState> {
    super::get_activity_state(&db, &msg.activity_id)
        .await
        .map_err(Into::into)
}

/// Pass activity state (which may include error details).
async fn set_activity_state_gsb(
    db: Arc<Mutex<DbExecutor>>,
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

async fn get_activity_usage_gsb(
    db: Arc<Mutex<DbExecutor>>,
    msg: GetActivityUsage,
) -> RpcMessageResult<GetActivityUsage> {
    super::get_activity_usage(&db, &msg.activity_id)
        .await
        .map_err(Error::into)
}

/// Pass current activity usage (which may include error details).
async fn set_activity_usage_gsb(
    db: Arc<Mutex<DbExecutor>>,
    msg: SetActivityUsage,
) -> RpcMessageResult<SetActivityUsage> {
    // TODO: caller authorization
    ActivityUsageDao::new(&db_conn!(db)?)
        .set(&msg.activity_id, &msg.usage.current_usage)
        .map_err(|e| Error::from(e).into())
}
