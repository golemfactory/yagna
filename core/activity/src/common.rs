use serde::Deserialize;
use uuid::Uuid;

use ya_core_model::{ethaddr::NodeId, market};
use ya_model::{
    activity::{ActivityState, ActivityUsage},
    market::Agreement,
};
use ya_persistence::executor::DbExecutor;
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use crate::dao::{ActivityDao, ActivityStateDao, ActivityUsageDao};
use crate::error::Error;

pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;
pub const DEFAULT_REQUEST_TIMEOUT: f32 = 12.0;

#[derive(Deserialize)]
pub struct PathActivity {
    pub activity_id: String,
}

#[derive(Deserialize)]
pub struct QueryTimeout {
    #[serde(rename = "timeout", default = "default_query_timeout")]
    pub timeout: Option<f32>,
}

#[derive(Deserialize, Debug)]
pub struct QueryTimeoutMaxCount {
    /// number of milliseconds to wait
    #[serde(rename = "timeout", default = "default_query_timeout")]
    pub timeout: Option<f32>,
    /// maximum count of events to return
    #[serde(rename = "maxCount")]
    pub max_count: Option<u32>,
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

pub(crate) async fn get_persisted_usage(
    db: &DbExecutor,
    activity_id: &str,
) -> Result<ActivityUsage, Error> {
    Ok(db.as_dao::<ActivityUsageDao>().get(&activity_id).await?)
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

pub(crate) async fn get_agreement(agreement_id: impl ToString) -> Result<Agreement, Error> {
    Ok(bus::service(market::BUS_ID)
        .send(market::GetAgreement {
            agreement_id: agreement_id.to_string(),
        })
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
) -> Result<Agreement, Error> {
    get_agreement(get_agreement_id(db, activity_id).await?).await
}

pub(crate) async fn authorize_activity_initiator(
    db: &DbExecutor,
    caller: impl ToString,
    activity_id: &str,
) -> Result<(), Error> {
    authorize_agreement_initiator(caller, &get_agreement_id(db, activity_id).await?).await
}

pub(crate) async fn authorize_activity_executor(
    db: &DbExecutor,
    caller: impl ToString,
    activity_id: &str,
) -> Result<(), Error> {
    authorize_agreement_executor(caller, &get_agreement_id(db, activity_id).await?).await
}

pub(crate) async fn authorize_agreement_initiator(
    caller: impl ToString,
    agreement_id: &str,
) -> Result<(), Error> {
    let agreement = get_agreement(agreement_id).await?;
    let initiator_id = agreement.requestor_id()?.parse()?;

    authorize_caller(caller.to_string().parse()?, initiator_id)
}

pub(crate) async fn authorize_agreement_executor(
    caller: impl ToString,
    agreement_id: &str,
) -> Result<(), Error> {
    let agreement = get_agreement(agreement_id).await?;
    let executor_id = agreement.provider_id()?.parse()?;

    authorize_caller(caller.to_string().parse()?, executor_id)
}

#[inline(always)]
pub(crate) fn authorize_caller(caller: NodeId, authorized: NodeId) -> Result<(), Error> {
    let msg = format!("caller: {} is not authorized: {}", caller, authorized);
    match caller == authorized {
        true => Ok(()),
        false => {
            log::debug!("{}", msg);
            Err(Error::Forbidden(msg))
        }
    }
}
