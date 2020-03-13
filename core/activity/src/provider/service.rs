use futures::prelude::*;
use std::convert::From;

use crate::common::{generate_id, get_agreement, parse_caller, RpcMessageResult};
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
    let requestor_id = parse_caller(&caller);
    let provider_id = get_provider_id(&msg.agreement_id, &requestor_id).await?;
    let activity_id = generate_id();

    db.as_dao::<ActivityDao>()
        .create(&activity_id, &provider_id, &msg.agreement_id)
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
    log::debug!("event inserted");

    let state = db
        .as_dao::<ActivityStateDao>()
        .get_future(&activity_id, &provider_id, None)
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
    log::info!("creating event for destroying activity");
    let requestor_id = parse_caller(&caller);
    let provider_id = get_provider_id(&msg.agreement_id, &requestor_id).await?;

    db.as_dao::<EventDao>()
        .create(
            &msg.activity_id,
            &provider_id,
            ActivityEventType::DestroyActivity,
        )
        .await
        .map_err(Error::from)?;

    log::info!(
        "waiting {:?}ms for activity status change to Terminate",
        msg.timeout
    );
    db.as_dao::<ActivityStateDao>()
        .get_future(
            &msg.activity_id,
            &provider_id,
            Some(StatePair(State::Terminated, None)),
        )
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
    let requestor_id = parse_caller(&caller);
    let provider_id = get_provider_id(&msg.agreement_id, &requestor_id).await?;

    super::get_activity_state(&db, &msg.activity_id, &provider_id)
        .await
        .map_err(Into::into)
}

/// Pass activity state (which may include error details).
/// Called by ExeUnits.
async fn set_activity_state_gsb(
    db: DbExecutor,
    _caller: String,
    msg: SetActivityState,
) -> RpcMessageResult<SetActivityState> {
    let agreement = get_agreement(&msg.agreement_id).await?;
    let provider_id = agreement
        .offer
        .provider_id
        .ok_or(Error::BadRequest("no provider id".into()))?;

    super::set_activity_state(&db, &msg.activity_id, &provider_id, msg.state)
        .map_err(Into::into)
        .await
}

async fn get_activity_usage_gsb(
    db: DbExecutor,
    caller: String,
    msg: GetActivityUsage,
) -> RpcMessageResult<GetActivityUsage> {
    let requestor_id = parse_caller(&caller);
    let provider_id = get_provider_id(&msg.agreement_id, &requestor_id).await?;

    super::get_activity_usage(&db, &provider_id, &msg.activity_id)
        .await
        .map_err(Error::into)
}

/// Pass current activity usage (which may include error details).
/// Called by ExeUnits.
async fn set_activity_usage_gsb(
    db: DbExecutor,
    _caller: String,
    msg: SetActivityUsage,
) -> RpcMessageResult<SetActivityUsage> {
    let agreement = get_agreement(&msg.agreement_id).await?;
    let provider_id = agreement
        .offer
        .provider_id
        .ok_or(Error::BadRequest("no provider id".into()))?;

    db.as_dao::<ActivityUsageDao>()
        .set(&msg.activity_id, &provider_id, &msg.usage.current_usage)
        .await
        .map_err(|e| Error::from(e).into())
}

async fn get_provider_id(agreement_id: &str, requestor_id: &str) -> Result<String, Error> {
    let agreement = get_agreement(agreement_id).await?;

    let expected_id = agreement
        .demand
        .requestor_id
        .ok_or(Error::BadRequest("no requestor id".into()))?;
    let provider_id = agreement
        .offer
        .provider_id
        .ok_or(Error::BadRequest("no provider id".into()))?;

    match requestor_id == expected_id {
        true => Ok(provider_id),
        false => Err(Error::Forbidden),
    }
}
