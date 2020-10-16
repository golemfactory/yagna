use actix_rt::Arbiter;
use chrono::Utc;
use futures::future::LocalBoxFuture;
use futures::prelude::*;
use metrics::{counter, gauge};
use std::convert::From;
use std::time::Duration;

use ya_client_model::activity::{ActivityState, ActivityUsage, State, StatePair};
use ya_core_model::activity;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::{timeout::*, typed::ServiceBinder};

use crate::common::{
    authorize_activity_initiator, authorize_agreement_initiator, generate_id,
    get_activity_agreement, get_agreement, get_persisted_state, get_persisted_usage,
    set_persisted_state, RpcMessageResult,
};
use crate::dao::*;
use crate::db::models::ActivityEventType;
use crate::error::Error;

const INACTIVITY_LIMIT_SECONDS_ENV_VAR: &str = "INACTIVITY_LIMIT_SECONDS";
const UNRESPONSIVE_LIMIT_SECONDS_ENV_VAR: &str = "UNRESPONSIVE_LIMIT_SECONDS";
const DEFAULT_INACTIVITY_LIMIT_SECONDS: f64 = 10.;
const DEFAULT_UNRESPONSIVE_LIMIT_SECONDS: f64 = 5.;
const MIN_INACTIVITY_LIMIT_SECONDS: f64 = 2.;
const MIN_UNRESPONSIVE_LIMIT_SECONDS: f64 = 2.;

#[inline]
fn inactivity_limit_seconds() -> f64 {
    seconds_limit(
        INACTIVITY_LIMIT_SECONDS_ENV_VAR,
        DEFAULT_INACTIVITY_LIMIT_SECONDS,
        MIN_INACTIVITY_LIMIT_SECONDS,
    )
}

#[inline]
fn unresponsive_limit_seconds() -> f64 {
    seconds_limit(
        UNRESPONSIVE_LIMIT_SECONDS_ENV_VAR,
        DEFAULT_UNRESPONSIVE_LIMIT_SECONDS,
        MIN_UNRESPONSIVE_LIMIT_SECONDS,
    )
}

fn seconds_limit(env_var: &str, default_val: f64, min_val: f64) -> f64 {
    let limit = std::env::var(env_var)
        .and_then(|v| v.parse().map_err(|_| std::env::VarError::NotPresent))
        .unwrap_or(default_val);
    limit.max(min_val)
}

pub fn bind_gsb(db: &DbExecutor) {
    // public for remote requestors interactions
    ServiceBinder::new(activity::BUS_ID, db, ())
        .bind(create_activity_gsb)
        .bind(destroy_activity_gsb)
        .bind(get_activity_state_gsb)
        .bind(get_activity_usage_gsb);

    // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
    // until first change to value will be made.
    counter!("activity.provider.created", 0);
    counter!("activity.provider.destroyed", 0);
    counter!("activity.provider.destroyed.by_requestor", 0);
    counter!("activity.provider.destroyed.unresponsive", 0);

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

    counter!("activity.provider.created", 1);

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
    let result = db
        .as_dao::<ActivityStateDao>()
        .get_state_wait(&msg.activity_id, vec![State::Terminated.into()])
        .timeout(msg.timeout)
        .map_err(Error::from)
        .await
        .map(|_| ())?;

    counter!("activity.provider.destroyed.by_requestor", 1);
    Ok(result)
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
    let limit_s = inactivity_limit_seconds();
    let unresp_s = unresponsive_limit_seconds();
    let delay = Duration::from_secs_f64(1.);
    let mut prev_state: Option<ActivityState> = None;

    log::debug!("Starting activity monitor: {}", activity_id);

    loop {
        if let Ok((state, usage)) = get_activity_progress(&db, &activity_id).await {
            if !state.state.alive() {
                break;
            }

            let dt = (Utc::now().timestamp() - usage.timestamp) as f64;
            if dt > limit_s {
                log::warn!("activity {} inactive for {}s, destroying", activity_id, dt);
                enqueue_destroy_evt(db, &activity_id, &provider_id).await;

                counter!("activity.provider.destroyed.unresponsive", 1);
                break;
            } else if state.state.0 != State::Unresponsive && dt >= unresp_s {
                log::warn!("activity {} unresponsive after {}s", activity_id, dt);
                let new_state = ActivityState::from(StatePair(State::Unresponsive, state.state.1));
                prev_state = Some(state);
                if let Err(e) = set_persisted_state(&db, &activity_id, new_state).await {
                    log::error!("cannot update activity {} state: {}", activity_id, e);
                }
            } else if state.state.0 == State::Unresponsive && dt < unresp_s {
                log::warn!("activity {} is now responsive", activity_id);
                let state = match prev_state.take() {
                    Some(state) => state,
                    _ => panic!("unknown pre-unresponsive state of activity {}", activity_id),
                };
                if let Err(e) = set_persisted_state(&db, &activity_id, state).await {
                    log::error!("cannot update activity {} state: {}", activity_id, e);
                }
            }
        };

        tokio::time::delay_for(delay).await;
    }

    // If we got here, we can be sure, that activity was already destroyed.
    // Counting activities in all other places can result with duplicated
    // DestroyActivity events.
    counter!("activity.provider.destroyed", 1);
    log::debug!("Stopping activity monitor: {}", activity_id);
}

/// Local Activity services for ExeUnit reporting.
mod local {
    use super::*;
    use crate::common::{set_persisted_state, set_persisted_usage};
    use ya_core_model::activity::local::StatsResult;

    pub fn bind_gsb(db: &DbExecutor) {
        ServiceBinder::new(activity::local::BUS_ID, db, ())
            .bind(set_activity_state_gsb)
            .bind(set_activity_usage_gsb)
            .bind(get_agreement_id_gsb)
            .bind(activity_status);
    }

    async fn activity_status(
        db: DbExecutor,
        _caller: String,
        _msg: activity::local::Stats,
    ) -> RpcMessageResult<activity::local::Stats> {
        let total = db
            .as_dao::<ActivityStateDao>()
            .stats()
            .await
            .map_err(Error::from)?;
        let last_1h = db
            .as_dao::<ActivityStateDao>()
            .stats_1h()
            .await
            .map_err(Error::from)?;

        Ok(StatsResult {
            total,
            last_1h,
            last_activity_ts: None,
        })
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
        if let Some(usage_vec) = &msg.usage.current_usage {
            let activity_id = msg.activity_id.clone();
            for (idx, value) in usage_vec.iter().enumerate() {
                gauge!(format!("activity.provider.usage.{}", idx), *value as i64, "activity_id" => activity_id.clone());
            }
        }

        set_persisted_usage(&db, &msg.activity_id, msg.usage).await?;
        Ok(())
    }

    /// Get agreement ID for a given activity ID
    /// Called e.g. by payment module
    async fn get_agreement_id_gsb(
        db: DbExecutor,
        _caller: String,
        msg: activity::local::GetAgreementId,
    ) -> RpcMessageResult<activity::local::GetAgreementId> {
        let agreement = get_activity_agreement(&db, &msg.activity_id).await?;
        Ok(agreement.agreement_id)
    }
}
