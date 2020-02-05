use futures::prelude::*;
use std::convert::From;

use crate::common::{generate_id, is_activity_initiator, is_agreement_initiator, RpcMessageResult};
use crate::dao::*;
use crate::db_conn;
use crate::error::Error;
use ya_core_model::activity::*;
use ya_model::activity::{provider_event::ProviderEventType, State};
use ya_persistence::executor::DbExecutor;
use ya_service_bus::timeout::IntoTimeoutFuture;

lazy_static::lazy_static! {
    static ref PRIVATE_ID: String = format!("/private{}", SERVICE_ID);
    static ref PUBLIC_ID: String = format!("/public{}", SERVICE_ID);
}

pub fn bind_gsb(db: &DbExecutor) {
    log::info!("activating activity provider service");

    // public for remote requestors interactions
    bind_gsb_method!(&PUBLIC_ID, db, create_activity_gsb);
    bind_gsb_method!(&PUBLIC_ID, db, destroy_activity_gsb);
    bind_gsb_method!(&PUBLIC_ID, db, get_activity_state_gsb);
    bind_gsb_method!(&PUBLIC_ID, db, get_activity_usage_gsb);

    // local for ExeUnit interactions
    bind_gsb_method!(&PRIVATE_ID, db, set_activity_state_gsb);
    bind_gsb_method!(&PRIVATE_ID, db, set_activity_usage_gsb);

    log::info!("activity provider service activated");
}

/// Creates new Activity based on given Agreement.
async fn create_activity_gsb(
    db: DbExecutor,
    caller: String,
    msg: CreateActivity,
) -> RpcMessageResult<CreateActivity> {
    let activity_id = generate_id();

    if !is_agreement_initiator(caller, msg.agreement_id.clone()).await? {
        return Err(Error::Forbidden.into());
    }

    {
        let activity_id = activity_id.clone();
        let agreement_id = msg.agreement_id.clone();
        db.with_transaction(move |conn| {
            ActivityDao::new(&conn)
                .create(&activity_id, &agreement_id)
                .map_err(Error::from)?;
            log::info!("activity inserted: {}", activity_id);
            EventDao::new(&conn)
                .create(
                    &activity_id,
                    serde_json::to_string(&ProviderEventType::CreateActivity)
                        .unwrap()
                        .as_str(),
                )
                .map_err(Error::from)?;
            log::info!("event inserted");
            Ok::<_, crate::error::Error>(())
        })
        .await?;
    }

    {
        let conn = db.conn().map_err(crate::error::Error::from)?;
        let state = ActivityStateDao::new(&conn)
            .get_future(&activity_id, None)
            .timeout(msg.timeout)
            .map_err(Error::from)
            .await?
            .map_err(Error::from)?;
        log::info!("activity state: {:?}", state);
    }

    Ok(activity_id)
}

/// Destroys given Activity.
async fn destroy_activity_gsb(
    db: DbExecutor,
    caller: String,
    msg: DestroyActivity,
) -> RpcMessageResult<DestroyActivity> {
    let conn = db_conn!(db)?;

    if !is_activity_initiator(&conn, caller, &msg.activity_id).await? {
        return Err(Error::Forbidden.into());
    }

    log::info!("creating event for destroying activity");
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
    db: DbExecutor,
    caller: String,
    msg: GetActivityState,
) -> RpcMessageResult<GetActivityState> {
    let conn = &db_conn!(db)?;
    if !is_activity_initiator(&conn, caller, &msg.activity_id).await? {
        return Err(Error::Forbidden.into());
    }

    super::get_activity_state(&conn, &msg.activity_id)
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
    let conn = db_conn!(db)?;
    if !is_activity_initiator(&conn, caller, &msg.activity_id).await? {
        return Err(Error::Forbidden.into());
    }

    super::set_activity_state(&conn, &msg.activity_id, msg.state)
        .map_err(Into::into)
        .await
}

async fn get_activity_usage_gsb(
    db: DbExecutor,
    caller: String,
    msg: GetActivityUsage,
) -> RpcMessageResult<GetActivityUsage> {
    let conn = &db_conn!(db)?;
    if !is_activity_initiator(&conn, caller, &msg.activity_id).await? {
        return Err(Error::Forbidden.into());
    }

    super::get_activity_usage(&conn, &msg.activity_id)
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
    let conn = db_conn!(db)?;
    if !is_activity_initiator(&conn, caller, &msg.activity_id).await? {
        return Err(Error::Forbidden.into());
    }

    ActivityUsageDao::new(&conn)
        .set(&msg.activity_id, &msg.usage.current_usage)
        .map_err(|e| Error::from(e).into())
}
