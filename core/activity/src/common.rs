use serde::Deserialize;
use uuid::Uuid;

use ya_core_model::market;

use ya_persistence::executor::ConnType;
use ya_service_bus::RpcEndpoint;
use ya_service_bus::RpcMessage;

use crate::dao::{ActivityDao, NotFoundAsOption};
use crate::error::Error;

use ya_model::market::Agreement;

use ya_service_bus::typed as bus;

pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;
pub const DEFAULT_REQUEST_TIMEOUT: u32 = 120 * 1000; // ms

#[derive(Deserialize)]
pub struct PathActivity {
    pub activity_id: String,
}

#[derive(Deserialize)]
pub struct QueryTimeout {
    #[serde(default = "default_query_timeout")]
    pub timeout: Option<u32>,
}

#[derive(Deserialize)]
pub struct QueryTimeoutMaxCount {
    #[serde(default = "default_query_timeout")]
    pub timeout: Option<u32>,
    #[serde(rename = "maxCount")]
    pub max_count: Option<u32>,
}

#[inline(always)]
pub(crate) fn default_query_timeout() -> Option<u32> {
    Some(DEFAULT_REQUEST_TIMEOUT)
}

#[inline(always)]
pub(crate) fn generate_id() -> String {
    // TODO: replace with a cryptographically secure generator
    Uuid::new_v4().to_simple().to_string()
}

pub(crate) fn into_json_response<T>(
    result: std::result::Result<T, Error>,
) -> actix_web::HttpResponse
where
    T: serde::Serialize,
{
    let result = match result {
        Ok(value) => serde_json::to_string(&value).map_err(Error::from),
        Err(e) => Err(e),
    };

    match result {
        Ok(value) => actix_web::HttpResponse::Ok()
            .content_type("application/json")
            .body(value)
            .into(),
        Err(e) => e.into(),
    }
}

pub(crate) async fn get_activity_agreement(
    conn: &ConnType,
    activity_id: &str,
    _timeout: Option<u32>,
) -> Result<Agreement, Error> {
    let agreement_id = ActivityDao::new(conn)
        .get_agreement_id(activity_id)
        .not_found_as_option()
        .map_err(Error::from)?
        .ok_or(Error::NotFound)?;

    let agreement = bus::service(market::BUS_ID)
        .send(market::GetAgreement { agreement_id })
        .await??;

    Ok(agreement)
}
