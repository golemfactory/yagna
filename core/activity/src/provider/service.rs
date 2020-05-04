use crate::common::{
    authorize_activity_initiator, authorize_agreement_initiator, generate_id, get_agreement,
    get_persisted_state, get_persisted_usage, RpcMessageResult,
};
use crate::dao::*;
use crate::error::Error;
use actix_rt::Arbiter;
use chrono::Utc;
use futures::future::LocalBoxFuture;
use futures::prelude::*;
use std::convert::From;
use std::time::Duration;
use ya_client_model::activity::{ActivityState, ActivityUsage, State};
use ya_core_model::activity;
use ya_persistence::executor::DbExecutor;
use ya_persistence::models::ActivityEventType;
use ya_service_bus::{timeout::*, typed::ServiceBinder};

const INACTIVITY_LIMIT_SECONDS_ENV_VAR: &str = "INACTIVITY_LIMIT_SECONDS";
const DEFAULT_INACTIVITY_LIMIT_SECONDS: i64 = 10;
const MIN_INACTIVITY_LIMIT_SECONDS: i64 = 2;

fn inactivity_limit_seconds() -> i64 {
    let limit = std::env::var(INACTIVITY_LIMIT_SECONDS_ENV_VAR)
        .and_then(|v| v.parse().map_err(|_| std::env::VarError::NotPresent))
        .unwrap_or(DEFAULT_INACTIVITY_LIMIT_SECONDS);
    std::cmp::max(limit, MIN_INACTIVITY_LIMIT_SECONDS)
}

pub fn bind_gsb(db: &DbExecutor) {
    // public for remote requestors interactions
    ServiceBinder::new(activity::BUS_ID, db, ())
        .bind(create_activity_gsb)
        .bind(destroy_activity_gsb)
        .bind(get_activity_state_gsb)
        .bind(get_activity_usage_gsb);

    local::bind_gsb(db);
}

/// Creates new Activity based on given Agreement.
async fn create_activity_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::Create,
) -> RpcMessageResult<activity::Create> {
    authorize_agreement_initiator(caller, &msg.agreement_id).await?;

    let activity_id = generate_id();
    let agreement = get_agreement(&msg.agreement_id).await?;
    let provider_id = agreement.provider_id().map_err(Error::from)?.to_string();

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

    db.as_dao::<ActivityStateDao>()
        .get_state_wait(
            &activity_id,
            vec![State::Initialized.into(), State::Terminated.into()],
        )
        .timeout(msg.timeout)
        .await
        .map_err(|e| {
            Arbiter::spawn(enqueue_destroy_evt(db.clone(), &activity_id, &provider_id));
            Error::from(e)
        })?
        .map_err(|e| {
            Arbiter::spawn(enqueue_destroy_evt(db.clone(), &activity_id, &provider_id));
            Error::from(e)
        })?;

    Arbiter::spawn(monitor_activity(db, activity_id.clone(), provider_id));
    Ok(activity_id)
}

/// Destroys given Activity.
async fn destroy_activity_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::Destroy,
) -> RpcMessageResult<activity::Destroy> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

    if !get_persisted_state(&db, &msg.activity_id).await?.alive() {
        return Ok(());
    }

    let agreement = get_agreement(&msg.agreement_id).await?;
    db.as_dao::<EventDao>()
        .create(
            &msg.activity_id,
            agreement.provider_id().map_err(Error::from)?,
            ActivityEventType::DestroyActivity,
        )
        .await
        .map_err(Error::from)?;

    log::debug!(
        "waiting {:?}ms for activity status change to Terminate",
        msg.timeout
    );
    Ok(db
        .as_dao::<ActivityStateDao>()
        .get_state_wait(&msg.activity_id, vec![State::Terminated.into()])
        .timeout(msg.timeout)
        .map_err(Error::from)
        .await
        .map(|_| ())?)
}

async fn get_activity_state_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::GetState,
) -> RpcMessageResult<activity::GetState> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

    Ok(get_persisted_state(&db, &msg.activity_id).await?)
}

async fn get_activity_usage_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::GetUsage,
) -> RpcMessageResult<activity::GetUsage> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

    Ok(get_persisted_usage(&db, &msg.activity_id).await?)
}

async fn get_activity_progress(
    db: &DbExecutor,
    activity_id: &str,
) -> Result<(ActivityState, ActivityUsage), Error> {
    let state = db.as_dao::<ActivityStateDao>().get(&activity_id).await?;
    let usage = db.as_dao::<ActivityUsageDao>().get(&activity_id).await?;
    Ok((state, usage))
}

fn enqueue_destroy_evt(
    db: DbExecutor,
    activity_id: impl ToString,
    provider_id: impl ToString,
) -> LocalBoxFuture<'static, ()> {
    let activity_id = activity_id.to_string();
    let provider_id = provider_id.to_string();

    log::debug!("Enqueueing a Destroy event for activity {}", activity_id);

    async move {
        if let Err(err) = db
            .as_dao::<EventDao>()
            .create(
                &activity_id,
                &provider_id,
                ActivityEventType::DestroyActivity,
            )
            .await
        {
            log::error!(
                "Unable to enqueue a Destroy event for activity {}: {:?}",
                activity_id,
                err
            );
        }
    }
    .boxed_local()
}

async fn monitor_activity(db: DbExecutor, activity_id: impl ToString, provider_id: impl ToString) {
    let activity_id = activity_id.to_string();
    let provider_id = provider_id.to_string();
    let limit_seconds = inactivity_limit_seconds();
    let delay = Duration::from_secs_f64(limit_seconds as f64 / 3.);

    log::debug!("Starting activity monitor: {}", activity_id);

    loop {
        if let Ok((state, usage)) = get_activity_progress(&db, &activity_id).await {
            if !state.state.alive() {
                break;
            }
            let inactive_seconds = Utc::now().timestamp() - usage.timestamp;
            if inactive_seconds > limit_seconds {
                log::warn!(
                    "activity {} inactive for {}s. Destroying...",
                    activity_id,
                    inactive_seconds
                );
                enqueue_destroy_evt(db, &activity_id, &provider_id).await;
                break;
            }
        };
        tokio::time::delay_for(delay).await;
    }

    log::debug!("Stopping activity monitor: {}", activity_id);
}

/// Local Activity services for ExeUnit reporting.
mod local {
    use super::*;
    use crate::common::{set_persisted_state, set_persisted_usage};

    pub fn bind_gsb(db: &DbExecutor) {
        ServiceBinder::new(activity::local::BUS_ID, db, ())
            .bind(set_activity_state_gsb)
            .bind(set_activity_usage_gsb);
    }

    /// Pass activity state (which may include error details).
    /// Called by ExeUnits.
    ///
    /// Security consideration: we assume activity_id as a cryptographically strong, so every1
    /// who knows it is authorized to call this endpoint
    async fn set_activity_state_gsb(
        db: DbExecutor,
        _caller: String,
        msg: activity::local::SetState,
    ) -> RpcMessageResult<activity::local::SetState> {
        set_persisted_state(&db, &msg.activity_id, msg.state).await?;
        Ok(())
    }

    /// Pass current activity usage (which may include error details).
    /// Called by ExeUnits.
    ///
    /// Security consideration: we assume activity_id as a cryptographically strong, so every1
    /// who knows it is authorized to call this endpoint
    async fn set_activity_usage_gsb(
        db: DbExecutor,
        _caller: String,
        msg: activity::local::SetUsage,
    ) -> RpcMessageResult<activity::local::SetUsage> {
        set_persisted_usage(&db, &msg.activity_id, msg.usage).await?;
        Ok(())
    }
}
