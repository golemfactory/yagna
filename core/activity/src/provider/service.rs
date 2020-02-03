use futures::prelude::*;
use std::convert::{From, TryInto};

use ya_core_model::activity::*;
use ya_model::activity::{provider_event::ProviderEventType, State};
use ya_persistence::executor::{ConnType, DbExecutor};
use ya_service_bus::timeout::IntoTimeoutFuture;

use crate::common::{fetch_agreement, generate_id, RpcMessageResult};
use crate::dao::*;
use crate::error::Error;
use ya_service_api::constants::NET_SERVICE_ID;

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
    let conn = db_conn!(db)?;

    let agreement = fetch_agreement(&msg.agreement_id).await?;
    log::info!("inserting agreement: {:#?}", agreement);
    AgreementDao::new(&conn)
        .create(agreement.try_into().map_err(Error::from)?)
        .map_err(Error::from)?;
    log::info!("agreement inserted");

    if !is_agreement_initiator(&conn, caller, &msg.agreement_id)? {
        return Err(Error::Forbidden.into());
    }

    ActivityDao::new(&conn)
        .create(&activity_id, &msg.agreement_id)
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

    let state = ActivityStateDao::new(&conn)
        .get_future(&activity_id, None)
        .timeout(msg.timeout)
        .map_err(Error::from)
        .await?
        .map_err(Error::from)?;
    log::info!("activity state: {:?}", state);

    Ok(activity_id)
}

/// Destroys given Activity.
async fn destroy_activity_gsb(
    db: DbExecutor,
    caller: String,
    msg: DestroyActivity,
) -> RpcMessageResult<DestroyActivity> {
    let conn = db_conn!(db)?;

    if !is_activity_owner(&conn, caller, &msg.activity_id)? {
        return Err(Error::Forbidden.into());
    }

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
    if !is_activity_owner(&conn, caller, &msg.activity_id)? {
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
    if !is_activity_owner(&conn, caller, &msg.activity_id)? {
        return Err(Error::Forbidden.into());
    }

    ActivityStateDao::new(&conn)
        .set(
            &msg.activity_id,
            msg.state.state.clone(),
            msg.state.reason.clone(),
            msg.state.error_message.clone(),
        )
        .map_err(|e| Error::from(e).into())
}

async fn get_activity_usage_gsb(
    db: DbExecutor,
    caller: String,
    msg: GetActivityUsage,
) -> RpcMessageResult<GetActivityUsage> {
    let conn = &db_conn!(db)?;
    if !is_activity_owner(&conn, caller, &msg.activity_id)? {
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
    if !is_activity_owner(&conn, caller, &msg.activity_id)? {
        return Err(Error::Forbidden.into());
    }

    ActivityUsageDao::new(&conn)
        .set(&msg.activity_id, &msg.usage.current_usage)
        .map_err(|e| Error::from(e).into())
}

fn is_activity_owner(
    conn: &ConnType,
    caller: String,
    activity_id: &str,
) -> std::result::Result<bool, Error> {
    let agreement_id = ActivityDao::new(&conn)
        .get_agreement_id(&activity_id)
        .map_err(Error::from)?;
    is_agreement_initiator(conn, caller, &agreement_id)
}

fn is_agreement_initiator(
    conn: &ConnType,
    caller: String,
    agreement_id: &str,
) -> std::result::Result<bool, Error> {
    let agreement = AgreementDao::new(&conn)
        .get(agreement_id)
        .map_err(Error::from)?;

    Ok(validate_caller(caller, agreement.demand_node_id))
}

#[inline(always)]
fn validate_caller(caller: String, expected: String) -> bool {
    // FIXME: impl a proper caller struct / parser
    let net_expected = format!("{}/{}", NET_SERVICE_ID, expected);
    log::info!("checking caller: {} vs expected: {}", caller, net_expected);
    caller == net_expected
}
