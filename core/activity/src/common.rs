use serde::Deserialize;
use serde_with::rust::string_empty_as_none;
use uuid::Uuid;

use ya_core_model::market;
use ya_model::market::Agreement;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use crate::dao::ActivityDao;
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
pub struct QueryTimeoutMaxEvents {
    /// number of milliseconds to wait
    #[serde(rename = "timeout", default = "default_query_timeout")]
    pub timeout: Option<f32>,
    /// maximum count of events to return
    #[serde(rename = "maxEvents", with = "string_empty_as_none", default)]
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

pub(crate) async fn get_agreement(agreement_id: impl ToString) -> Result<Agreement, Error> {
    Ok(bus::service(market::BUS_ID)
        .send(market::GetAgreement {
            agreement_id: agreement_id.to_string(),
        })
        .await??)
}

pub(crate) async fn get_activity_agreement(
    db: &DbExecutor,
    activity_id: &str,
    _timeout: Option<f32>,
) -> Result<Agreement, Error> {
    let agreement_id = db
        .as_dao::<ActivityDao>()
        .get_agreement_id(activity_id)
        .await
        .map_err(Error::from)?
        .ok_or(Error::NotFound)?;

    let agreement = bus::service(market::BUS_ID)
        .send(market::GetAgreement { agreement_id })
        .await??;

    Ok(agreement)
}

pub(crate) async fn authorize_activity_initiator(
    db: &DbExecutor,
    caller: impl ToString,
    activity_id: &str,
) -> Result<(), Error> {
    let agreement_id = db
        .as_dao::<ActivityDao>()
        .get_agreement_id(&activity_id)
        .await
        .map_err(Error::from)?
        .ok_or(Error::NotFound)?;
    authorize_agreement_initiator(caller, &agreement_id).await
}

pub(crate) async fn authorize_activity_executor(
    db: &DbExecutor,
    caller: impl ToString,
    activity_id: &str,
) -> Result<(), Error> {
    let agreement_id = db
        .as_dao::<ActivityDao>()
        .get_agreement_id(&activity_id)
        .await
        .map_err(Error::from)?
        .ok_or(Error::NotFound)?;
    authorize_agreement_executor(caller, &agreement_id).await
}

pub(crate) async fn authorize_agreement_initiator(
    caller: impl ToString,
    agreement_id: &str,
) -> Result<(), Error> {
    let agreement = get_agreement(agreement_id).await?;
    let initiator_id = agreement.demand.requestor_id()?;

    authorize_caller(caller, initiator_id)
}

pub(crate) async fn authorize_agreement_executor(
    caller: impl ToString,
    agreement_id: &str,
) -> Result<(), Error> {
    let agreement = get_agreement(agreement_id).await?;
    let executor_id = agreement.offer.provider_id()?;

    authorize_caller(caller, executor_id)
}

#[inline(always)]
pub(crate) fn authorize_caller(caller: impl ToString, authorized: &str) -> Result<(), Error> {
    let caller = caller.to_string();
    let authorized = authorized.to_string();
    log::debug!("caller {} vs {} authorized", caller, authorized);
    match caller == authorized {
        true => Ok(()),
        false => Err(Error::Forbidden),
    }
}
