use serde::Deserialize;
use uuid::Uuid;

use ya_client::{market::MarketProviderApi, web::WebClient};
use ya_core_model::appkey;
use ya_model::market::Agreement;
use ya_service_bus::{actix_rpc, RpcMessage};

use crate::error::Error;

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

pub(crate) async fn fetch_agreement(agreement_id: &String) -> Result<Agreement, Error> {
    log::info!("fetching appkey for default id");
    let app_key: appkey::AppKey = actix_rpc::service(appkey::BUS_ID)
        .send(appkey::Get::default())
        .await
        .unwrap() // FIXME
        .unwrap(); // FIXME
    log::info!("using appkey: {:?}", app_key);

    let market_api: MarketProviderApi = WebClient::with_token(&app_key.key)?.interface()?;
    log::info!("fetching agreement");
    Ok(market_api.get_agreement(agreement_id).await?)
}
