use serde::Deserialize;
use uuid::Uuid;

use ya_core_model::market;

use ya_persistence::executor::DbExecutor;
use ya_service_api::constants::NET_SERVICE_ID;
use ya_service_bus::{RpcEndpoint, RpcMessage};

use crate::dao::ActivityDao;
use crate::error::Error;

use ya_model::market::Agreement;

use ya_service_bus::typed as bus;

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

pub(crate) async fn get_agreement(agreement_id: &str) -> Result<Agreement, Error> {
    Ok(bus::service(market::BUS_ID)
        .send(market::GetAgreement {
            agreement_id: agreement_id.to_string(),
        })
        .await??)
}

pub(crate) async fn get_activity_agreement(
    db: &DbExecutor,
    activity_id: &str,
    identity_id: &str,
    _timeout: Option<f32>,
) -> Result<Agreement, Error> {
    let agreement_id = db
        .as_dao::<ActivityDao>()
        .get_agreement_id(activity_id, identity_id)
        .await
        .map_err(Error::from)?
        .ok_or(Error::NotFound)?;

    let agreement = bus::service(market::BUS_ID)
        .send(market::GetAgreement { agreement_id })
        .await??;

    Ok(agreement)
}

pub(crate) async fn authorize_agreement_initiator(
    caller: &str,
    agreement_id: &str,
) -> Result<(), Error> {
    let agreement = get_agreement(agreement_id).await?;
    let initiator_id = agreement
        .demand
        .requestor_id
        .ok_or(Error::BadRequest("no requestor id".into()))?;

    authorize_caller(caller, initiator_id)
}

#[inline(always)]
pub(crate) fn parse_caller(caller: &str) -> String {
    // FIXME: impl a proper caller struct / parser
    let pat = format!("{}/", NET_SERVICE_ID);
    caller.to_string().replacen(&pat, "", 1)
}

#[inline(always)]
pub(crate) fn authorize_caller(caller: &str, authorized: String) -> Result<(), Error> {
    let caller = parse_caller(caller);
    log::debug!("checking caller: {} vs expected: {}", caller, authorized);
    match caller == authorized {
        true => Ok(()),
        false => Err(Error::Forbidden),
    }
}
