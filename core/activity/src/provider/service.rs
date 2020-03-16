use futures::prelude::*;
use std::convert::From;

use crate::common::{
    authorize_activity_initiator, authorize_agreement_initiator, generate_id, get_agreement,
    RpcMessageResult,
};
use crate::dao::*;
use crate::error::Error;
use ya_core_model::activity::*;
use ya_model::activity::{activity_state::StatePair, State};
use ya_persistence::executor::DbExecutor;
use ya_persistence::models::ActivityEventType;
use ya_service_bus::timeout::*;

lazy_static::lazy_static! {
    static ref PRIVATE_ID: String = format!("/private{}", SERVICE_ID);
    static ref PUBLIC_ID: String = format!("/public{}", SERVICE_ID);
}

pub fn bind_gsb(db: &DbExecutor) {
    // public for remote requestors interactions
    bind_gsb_method!(&PUBLIC_ID, db, create_activity_gsb);
    bind_gsb_method!(&PUBLIC_ID, db, destroy_activity_gsb);
    bind_gsb_method!(&PUBLIC_ID, db, get_activity_state_gsb);
    bind_gsb_method!(&PUBLIC_ID, db, get_activity_usage_gsb);

    // local for ExeUnit interactions
    bind_gsb_method!(&PRIVATE_ID, db, set_activity_state_gsb);
    bind_gsb_method!(&PRIVATE_ID, db, set_activity_usage_gsb);
}

/// Creates new Activity based on given Agreement.
async fn create_activity_gsb(
    db: DbExecutor,
    caller: String,
    msg: CreateActivity,
) -> RpcMessageResult<CreateActivity> {
    authorize_agreement_initiator(caller, &msg.agreement_id).await?;

    let activity_id = generate_id();
    let provider_id = get_agreement(&msg.agreement_id)
        .await?
        .offer
        .provider_id
        .ok_or(Error::BadRequest("Invalid agreement".to_owned()))?;

    db.as_dao::<ActivityDao>()
        .create_if_not_exists(&activity_id, &msg.agreement_id)
        .await
        .map_err(Error::from)?;

    log::debug!("activity inserted: {}", activity_id);

    db.as_dao::<EventDao>()
        .create(
            &activity_id,
            &provider_id,
            ActivityEventType::CreateActivity,
        )
        .await
        .map_err(Error::from)?;

    let state = db
        .as_dao::<ActivityStateDao>()
        .get_wait(&activity_id, None)
        .timeout(msg.timeout)
        .map_err(Error::from)
        .await?
        .map_err(Error::from)?;
    log::debug!("activity state: {:?}", state);

    Ok(activity_id)
}

/// Destroys given Activity.
async fn destroy_activity_gsb(
    db: DbExecutor,
    caller: String,
    msg: DestroyActivity,
) -> RpcMessageResult<DestroyActivity> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;
    let provider_id = get_agreement(&msg.agreement_id)
        .await?
        .offer
        .provider_id
        .ok_or(Error::BadRequest("Invalid agreement".to_owned()))?;

    db.as_dao::<EventDao>()
        .create(
            &msg.activity_id,
            &provider_id,
            ActivityEventType::DestroyActivity,
        )
        .await
        .map_err(Error::from)?;

    log::debug!(
        "waiting {:?}ms for activity status change to Terminate",
        msg.timeout
    );
    db.as_dao::<ActivityStateDao>()
        .get_wait(&msg.activity_id, Some(StatePair(State::Terminated, None)))
        .timeout(msg.timeout)
        .map_err(Error::from)
        .await?
        .map_err(Error::from)?;

    Ok(())
}

async fn get_activity_state_gsb(
    db: DbExecutor,
    caller: String,
    msg: GetActivityState,
) -> RpcMessageResult<GetActivityState> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

    super::get_activity_state(&db, &msg.activity_id)
        .await
        .map_err(Into::into)
}

/// Pass activity state (which may include error details).
/// Called by ExeUnits.
async fn set_activity_state_gsb(
    db: DbExecutor,
    caller: String,
    msg: SetActivityState,
) -> RpcMessageResult<SetActivityState> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

    super::set_activity_state(&db, &msg.activity_id, msg.state)
        .map_err(Into::into)
        .await
}

async fn get_activity_usage_gsb(
    db: DbExecutor,
    caller: String,
    msg: GetActivityUsage,
) -> RpcMessageResult<GetActivityUsage> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

    super::get_activity_usage(&db, &msg.activity_id)
        .await
        .map_err(Error::into)
}

/// Pass current activity usage (which may include error details).
/// Called by ExeUnits.
async fn set_activity_usage_gsb(
    db: DbExecutor,
    caller: String,
    msg: SetActivityUsage,
) -> RpcMessageResult<SetActivityUsage> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

    db.as_dao::<ActivityUsageDao>()
        .set(&msg.activity_id, &msg.usage.current_usage)
        .await
        .map_err(|e| Error::from(e).into())
}
