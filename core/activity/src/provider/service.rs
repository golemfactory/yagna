#![allow(clippy::let_unit_value)]

use chrono::Utc;
use futures::future::LocalBoxFuture;
use futures::prelude::*;
use metrics::{counter, gauge};
use std::convert::From;
use std::time::Duration;

use ya_client_model::activity::{ActivityState, ActivityUsage, State, StatePair};
use ya_client_model::market::{agreement::State as AgreementState, Role};
use ya_client_model::NodeId;
use ya_core_model::activity::local::Credentials;
use ya_core_model::activity::RpcMessageError;
use ya_core_model::market::Agreement;
use ya_core_model::{activity, market};
use ya_persistence::executor::DbExecutor;

use ya_service_bus::{timeout::*, typed::ServiceBinder};
use ya_service_bus::{typed as bus, RpcEndpoint};

use crate::common::{
    authorize_activity_initiator, authorize_agreement_initiator, generate_id,
    get_activities_for_agreement, get_activity_agreement, get_agreement, get_agreements_by_state,
    get_persisted_state, get_persisted_usage, is_responsive, set_persisted_state, RpcMessageResult,
};
use crate::dao::*;
use crate::db::models::ActivityEventType;
use crate::error::Error;
use crate::TrackerRef;

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

pub fn bind_gsb(db: &DbExecutor, tracker: TrackerRef) {
    // public for remote requestors interactions
    ServiceBinder::new(activity::BUS_ID, db, tracker.clone())
        .bind_with_processor(create_activity_gsb)
        .bind(destroy_activity_gsb)
        .bind(get_activity_state_gsb)
        .bind(get_activity_usage_gsb);

    // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
    // until first change to value will be made.
    counter!("activity.provider.created", 0);
    counter!("activity.provider.create.agreement.not-approved", 0);
    counter!("activity.provider.destroyed", 0);
    counter!("activity.provider.unresponsive", 0);
    counter!("activity.provider.responsive-again", 0);
    counter!("activity.provider.destroyed.by_requestor", 0);
    counter!("activity.provider.destroyed.unresponsive", 0);
    counter!("activity.provider.events.query", 0);

    local::bind_gsb(&db.clone(), tracker.clone());

    tokio::task::spawn_local(initial_checkup(db.clone(), tracker.clone()));
}

async fn initial_checkup(db: DbExecutor, tracker: TrackerRef) {
    log::info!("Checkup for alive Activities remaining from previous runs");
    let agreements = match get_agreements_by_state(AgreementState::Approved).await {
        Ok(agreements) => agreements,
        Err(err) => {
            log::error!("Acitivties checkup error. Failed to list Agreements. Err: {err}");
            return;
        }
    };

    for agreement in agreements
        .into_iter()
        .filter(|agreement| agreement.role == Role::Provider)
    {
        let agreement = match bus::service(market::BUS_ID)
            .send(market::GetAgreement::as_provider(agreement.id))
            .await
        {
            Ok(Ok(agreement)) => agreement,
            Ok(Err(err)) => {
                log::error!("Acitivties checkup error. Failed to get Agreement. Err: {err}");
                continue;
            }
            Err(err) => {
                log::error!("Acitivties checkup error. GSB call failed. Err: {err}");
                continue;
            }
        };
        if let Err(err) = monitor_alive_activities(agreement, db.clone(), tracker.clone()).await {
            log::error!("Acitivties initial checkup error. Err: {err}")
        };
    }
}

async fn monitor_alive_activities(
    agreement: Agreement,
    db: DbExecutor,
    tracker: TrackerRef,
) -> anyhow::Result<()> {
    log::info!(
        "Checkup of Agreement {} for remaining alive Activities",
        agreement.agreement_id
    );
    let activities_ids = get_activities_for_agreement(&db, &agreement.agreement_id).await?;
    let activity_state_dao = db.as_dao::<ActivityStateDao>();
    for activity_id in activities_ids {
        let activity_state = activity_state_dao.get(&activity_id).await?;
        if is_responsive(activity_state) {
            log::info!("Spawning monitoring of Activity {activity_id}");
            tokio::task::spawn_local(monitor_activity(
                db.clone(),
                tracker.clone(),
                activity_id,
                *agreement.provider_id(),
                None,
            ));
        }
    }
    Ok(())
}

/// Creates new Activity based on given Agreement.
async fn create_activity_gsb(
    db: DbExecutor,
    mut tracker: TrackerRef,
    caller: String,
    msg: activity::Create,
) -> RpcMessageResult<activity::Create> {
    authorize_agreement_initiator(caller.clone(), &msg.agreement_id, Role::Provider).await?;

    let activity_id = generate_id();
    let agreement = get_agreement(&msg.agreement_id, Role::Provider).await?;
    let agreement_id = agreement.agreement_id.clone();
    let app_session_id = agreement.app_session_id.clone();

    if agreement.state != AgreementState::Approved {
        // to track inconsistencies between this and remote market service
        counter!("activity.provider.create.agreement.not-approved", 1);

        let msg = format!(
            "Agreement {} is not Approved. Current state: {:?}",
            msg.agreement_id, agreement.state
        );

        log::warn!("{}", msg);
        // below err would also pop up in requestor's corresponding create_activity
        return Err(RpcMessageError::BadRequest(msg));
    }

    db.as_dao::<ActivityDao>()
        .create_if_not_exists(&activity_id, &msg.agreement_id)
        .await
        .map_err(Error::from)?;

    log::debug!("activity inserted: {}", activity_id);

    db.as_dao::<EventDao>()
        .create(
            &activity_id,
            agreement.provider_id(),
            ActivityEventType::CreateActivity,
            msg.requestor_pub_key,
            &agreement.app_session_id,
        )
        .await
        .map_err(Error::from)?;

    if let Err(e) = tracker.start_activity(&activity_id, &agreement).await {
        log::error!("fail to notify on activity start. {:?}", e);
    }

    let credentials = activity_credentials(
        db.clone(),
        tracker.clone(),
        &activity_id,
        *agreement.provider_id(),
        app_session_id.clone(),
        msg.timeout,
    )
    .await
    .inspect_err(|_e| {
        tokio::task::spawn_local(enqueue_destroy_evt(
            db.clone(),
            tracker.clone(),
            &activity_id,
            *agreement.provider_id(),
            app_session_id,
        ));
    })?;

    log::info!(
        "Requestor [{caller}] created Activity [{activity_id}] for Agreement [{agreement_id}]"
    );
    counter!("activity.provider.created", 1);

    Ok(if credentials.is_none() {
        activity::CreateResponseCompat::ActivityId(activity_id)
    } else {
        activity::CreateResponse {
            activity_id,
            credentials,
        }
        .into()
    })
}

async fn activity_credentials(
    db: DbExecutor,
    tracker: TrackerRef,
    activity_id: &String,
    provider_id: NodeId,
    app_session_id: Option<String>,
    timeout: Option<f32>,
) -> Result<Option<Credentials>, Error> {
    let activity_state = db
        .as_dao::<ActivityStateDao>()
        .get_state_wait(
            activity_id,
            vec![State::Initialized.into(), State::Terminated.into()],
        )
        .timeout(timeout)
        .await??;

    if !activity_state.state.alive() {
        let reason = activity_state
            .reason
            .unwrap_or_else(|| "<unspecified>".into());
        let error = activity_state
            .error_message
            .unwrap_or_else(|| "<none>".into());
        let msg = format!(
            "Activity {} was abruptly terminated. Reason: {}, message: {}",
            activity_id, reason, error
        );
        return Err(Error::Service(msg));
    }

    tokio::task::spawn_local(monitor_activity(
        db.clone(),
        tracker,
        activity_id.clone(),
        provider_id,
        app_session_id,
    ));

    let credentials = db
        .as_dao::<ActivityCredentialsDao>()
        .get(activity_id)
        .await?
        .map(|c| serde_json::from_str(&c.credentials).map_err(|e| Error::Service(e.to_string())))
        .transpose()?;

    Ok(credentials)
}

/// Destroys given Activity.
async fn destroy_activity_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::Destroy,
) -> RpcMessageResult<activity::Destroy> {
    authorize_activity_initiator(&db, caller.clone(), &msg.activity_id, Role::Provider).await?;

    if !get_persisted_state(&db, &msg.activity_id).await?.alive() {
        return Ok(());
    }

    let agreement = get_agreement(&msg.agreement_id, Role::Provider).await?;
    db.as_dao::<EventDao>()
        .create(
            &msg.activity_id,
            agreement.provider_id(),
            ActivityEventType::DestroyActivity,
            None,
            &agreement.app_session_id,
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

    log::info!(
        "Requestor [{caller}] destroyed Activity [{}] for Agreement [{}]",
        msg.activity_id,
        agreement.agreement_id
    );

    counter!("activity.provider.destroyed.by_requestor", 1);
    Ok(result)
}

async fn get_activity_state_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::GetState,
) -> RpcMessageResult<activity::GetState> {
    authorize_activity_initiator(&db, caller, &msg.activity_id, Role::Provider).await?;

    Ok(get_persisted_state(&db, &msg.activity_id).await?)
}

async fn get_activity_usage_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::GetUsage,
) -> RpcMessageResult<activity::GetUsage> {
    authorize_activity_initiator(&db, caller, &msg.activity_id, Role::Provider).await?;

    Ok(get_persisted_usage(&db, &msg.activity_id).await?)
}

async fn get_activity_progress(
    db: &DbExecutor,
    activity_id: &str,
) -> Result<(ActivityState, ActivityUsage), Error> {
    let state = db.as_dao::<ActivityStateDao>().get(activity_id).await?;
    let usage = db.as_dao::<ActivityUsageDao>().get(activity_id).await?;
    Ok((state, usage))
}

fn enqueue_destroy_evt(
    db: DbExecutor,
    mut tracker: TrackerRef,
    activity_id: impl ToString,
    provider_id: NodeId,
    app_session_id: Option<String>,
) -> LocalBoxFuture<'static, ()> {
    let activity_id = activity_id.to_string();

    log::debug!("Enqueueing a Destroy event for activity {}", activity_id);

    async move {
        if let Err(err) = tracker.stop_activity(activity_id.clone()).await {
            log::error!(
                "failed to notify activity {} destruction. {:?}",
                &activity_id,
                err
            );
        }

        if let Err(err) = db
            .as_dao::<EventDao>()
            .create(
                &activity_id,
                &provider_id,
                ActivityEventType::DestroyActivity,
                None,
                &app_session_id,
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

async fn monitor_activity(
    db: DbExecutor,
    mut tracker: TrackerRef,
    activity_id: impl ToString,
    provider_id: NodeId,
    app_session_id: Option<String>,
) {
    let activity_id = activity_id.to_string();
    let inactivity_limit = inactivity_limit_seconds();
    let unresponsive_limit = unresponsive_limit_seconds();
    let delay = Duration::from_secs_f64(1.);
    let mut prev_state: Option<ActivityState> = None;

    log::debug!("Starting activity monitor: {}", activity_id);

    loop {
        if let Ok((state, usage)) = get_activity_progress(&db, &activity_id).await {
            if !state.state.alive() {
                let _ = tracker.stop_activity(activity_id.clone()).await;
                break;
            }

            let since_update = (Utc::now().timestamp() - usage.timestamp) as f64;

            if since_update > unresponsive_limit {
                if state.state.0 != State::Unresponsive {
                    log::warn!(
                        "activity {} unresponsive after {}s",
                        activity_id,
                        since_update
                    );
                    let new_state =
                        ActivityState::from(StatePair(State::Unresponsive, state.state.1));
                    prev_state = Some(state);
                    let _ = tracker
                        .update_state(activity_id.clone(), State::Unresponsive)
                        .await;
                    if let Err(e) = set_persisted_state(&db, &activity_id, new_state).await {
                        log::error!("cannot update activity {} state: {}", activity_id, e);
                    }
                    counter!("activity.provider.unresponsive", 1);
                }

                // `unresponsive_limit` should be configured to be smaller than `inactivity_limit`
                if since_update > inactivity_limit {
                    log::warn!(
                        "activity {} inactive for {}s, destroying",
                        activity_id,
                        since_update
                    );
                    enqueue_destroy_evt(
                        db,
                        tracker.clone(),
                        &activity_id,
                        provider_id,
                        app_session_id.clone(),
                    )
                    .await;

                    counter!("activity.provider.destroyed.unresponsive", 1);
                    break;
                }
            } else if state.state.0 == State::Unresponsive && since_update <= unresponsive_limit {
                log::warn!("activity {} is now responsive", activity_id);
                let Some(state) = prev_state.take() else {
                    log::error!("Unknown pre-unresponsive state of activity {activity_id}");
                    break;
                };
                let _ = tracker
                    .update_state(activity_id.clone(), state.state.0)
                    .await;
                if let Err(e) = set_persisted_state(&db, &activity_id, state).await {
                    log::error!("cannot update activity {} state: {}", activity_id, e);
                }
                counter!("activity.provider.responsive-again", 1);
            }
        };

        tokio::time::sleep(delay).await;
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

    pub fn bind_gsb(db: &DbExecutor, tracker: TrackerRef) {
        ServiceBinder::new(activity::local::BUS_ID, db, tracker)
            .bind_with_processor(set_activity_state_gsb)
            .bind_with_processor(set_activity_usage_gsb)
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

        let map_entry = |(k, v): (State, _)| (format!("{:?}", k), v);
        Ok(StatsResult {
            total: total.into_iter().map(map_entry).collect(),
            last_1h: last_1h.into_iter().map(map_entry).collect(),
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
        mut tracker: TrackerRef,
        _caller: String,
        msg: activity::local::SetState,
    ) -> RpcMessageResult<activity::local::SetState> {
        if let Some(credentials) = msg.credentials {
            db.as_dao::<ActivityCredentialsDao>()
                .set(&msg.activity_id, credentials)
                .await
                .map_err(Error::from)?;
        }
        let _ = tracker
            .update_state(msg.activity_id.clone(), msg.state.state.0)
            .await;
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
        mut tracker: TrackerRef,
        _caller: String,
        msg: activity::local::SetUsage,
    ) -> RpcMessageResult<activity::local::SetUsage> {
        if let Some(usage_vec) = &msg.usage.current_usage {
            let activity_id = msg.activity_id.clone();
            for (idx, value) in usage_vec.iter().enumerate() {
                gauge!(format!("activity.provider.usage.{}", idx), *value as i64, "activity_id" => activity_id.clone());
            }
            // ignore
            let _ = tracker
                .update_counters(activity_id.clone(), usage_vec.clone())
                .await;
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
        let agreement = get_activity_agreement(&db, &msg.activity_id, msg.role).await?;
        Ok(agreement.agreement_id)
    }
}
