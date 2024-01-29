use crate::dao::{ActivityDao, ActivityStateDao, ActivityUsageDao};
use crate::error::Error;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::time::Duration;
use uuid::Uuid;

use ya_client_model::{
    activity::{ActivityState, ActivityUsage},
    market::{Agreement, Role},
    NodeId,
};
use ya_core_model::{activity, market};
use ya_net::RemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::typed::Endpoint;
use ya_service_bus::{timeout::IntoDuration, typed as bus, RpcEndpoint, RpcMessage};

pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;
pub const DEFAULT_REQUEST_TIMEOUT: f32 = 5.0;
const DEFAULT_TIMEOUT_MARGIN: f32 = 1.0;

#[derive(Deserialize)]
pub struct PathActivity {
    pub activity_id: String,
}

#[derive(Deserialize)]
pub struct PathActivityUrl {
    pub activity_id: String,
    pub url: String,
}

#[derive(Deserialize)]
pub struct QueryTimeout {
    #[serde(rename = "timeout", default = "default_query_timeout")]
    pub timeout: Option<f32>,
}

#[derive(Deserialize)]
pub struct QueryTimeoutCommandIndex {
    #[serde(rename = "timeout")]
    pub timeout: Option<f32>,
    #[serde(rename = "commandIndex")]
    pub command_index: Option<usize>,
}

#[derive(Deserialize, Debug)]
pub struct QueryEvents {
    /// application session identifier
    #[serde(rename = "appSessionId")]
    pub app_session_id: Option<String>,
    /// number of milliseconds to wait
    #[serde(rename = "timeout", default = "default_query_timeout")]
    pub timeout: Option<f32>,
    /// select events past the specified point in time
    #[serde(rename = "afterTimestamp")]
    pub after_timestamp: DateTime<Utc>,
    /// maximum count of events to return
    #[serde(rename = "maxEvents", default)]
    pub max_events: Option<u32>,
}

#[inline(always)]
pub(crate) fn default_query_timeout() -> Option<f32> {
    Some(DEFAULT_REQUEST_TIMEOUT)
}

#[inline(always)]
pub(crate) fn generate_id() -> String {
    // TODO: replace with a cryptographically secure generator
    Uuid::new_v4().to_simple().to_string()
}

pub(crate) async fn _get_activities(db: &DbExecutor) -> Result<Vec<String>, Error> {
    Ok(db.as_dao::<ActivityDao>()._get_activity_ids().await?)
}

pub(crate) async fn get_persisted_state(
    db: &DbExecutor,
    activity_id: &str,
) -> Result<ActivityState, Error> {
    Ok(db.as_dao::<ActivityStateDao>().get(activity_id).await?)
}

pub(crate) async fn set_persisted_state(
    db: &DbExecutor,
    activity_id: &str,
    activity_state: ActivityState,
) -> Result<ActivityState, Error> {
    Ok(db
        .as_dao::<ActivityStateDao>()
        .set(activity_id, activity_state)
        .await?)
}

pub(crate) fn agreement_provider_service(
    id: &Identity,
    agreement: &Agreement,
) -> Result<Endpoint, Error> {
    Ok(ya_net::from(id.identity)
        .to(*agreement.provider_id())
        .service(activity::BUS_ID))
}

pub(crate) async fn get_persisted_usage(
    db: &DbExecutor,
    activity_id: &str,
) -> Result<ActivityUsage, Error> {
    Ok(db.as_dao::<ActivityUsageDao>().get(activity_id).await?)
}

pub(crate) async fn set_persisted_usage(
    db: &DbExecutor,
    activity_id: &str,
    activity_usage: ActivityUsage,
) -> Result<ActivityUsage, Error> {
    Ok(db
        .as_dao::<ActivityUsageDao>()
        .set(activity_id, activity_usage)
        .await?)
}

pub(crate) async fn get_agreement(
    agreement_id: impl ToString,
    role: Role,
) -> Result<Agreement, Error> {
    Ok(bus::service(market::BUS_ID)
        .send(market::GetAgreement::as_role(
            agreement_id.to_string(),
            role,
        ))
        .await??)
}

pub(crate) async fn get_agreement_id(db: &DbExecutor, activity_id: &str) -> Result<String, Error> {
    Ok(db
        .as_dao::<ActivityDao>()
        .get_agreement_id(activity_id)
        .await?)
}

pub(crate) async fn get_activity_agreement(
    db: &DbExecutor,
    activity_id: &str,
    role: Role,
) -> Result<Agreement, Error> {
    get_agreement(get_agreement_id(db, activity_id).await?, role).await
}

pub(crate) async fn authorize_activity_initiator(
    db: &DbExecutor,
    caller: impl ToString,
    activity_id: &str,
    role: Role,
) -> Result<(), Error> {
    authorize_agreement_initiator(caller, &get_agreement_id(db, activity_id).await?, role).await
}

pub(crate) async fn authorize_activity_executor(
    db: &DbExecutor,
    caller: impl ToString,
    activity_id: &str,
    role: Role,
) -> Result<(), Error> {
    authorize_agreement_executor(caller, &get_agreement_id(db, activity_id).await?, role).await
}

pub(crate) async fn authorize_agreement_initiator(
    caller: impl ToString,
    agreement_id: &str,
    role: Role,
) -> Result<(), Error> {
    let agreement = get_agreement(agreement_id, role).await?;

    authorize_caller(&caller.to_string().parse()?, agreement.requestor_id())
}

pub(crate) async fn authorize_agreement_executor(
    caller: impl ToString,
    agreement_id: &str,
    role: Role,
) -> Result<(), Error> {
    let agreement = get_agreement(agreement_id, role).await?;

    authorize_caller(&caller.to_string().parse()?, agreement.provider_id())
}

#[inline(always)]
pub(crate) fn authorize_caller(caller: &NodeId, authorized: &NodeId) -> Result<(), Error> {
    let msg = format!("caller: {} is not authorized: {}", caller, authorized);
    match caller == authorized {
        true => Ok(()),
        false => {
            log::debug!("{}", msg);
            Err(Error::Forbidden(msg))
        }
    }
}

pub(crate) fn timeout_margin<D: IntoDuration>(timeout: Option<D>) -> Option<Duration> {
    timeout.map(|t| t.into_duration() + Duration::from_secs_f32(DEFAULT_TIMEOUT_MARGIN))
}
