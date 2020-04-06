use anyhow::anyhow;
use std::rc::Rc;
use std::time::Duration;
use tokio::time::delay_for;
use url::Url;

use ya_client::{
    market::MarketProviderApi,
    web::{WebClient, WebInterface},
};
use ya_core_model::{appkey, market};
use ya_service_api_interfaces::Service;
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use crate::Error;

const KEY_EXPORT_RETRY_COUNT: u8 = 3;
const KEY_EXPORT_RETRY_DELAY: Duration = Duration::from_secs(5);

pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;

pub struct MarketService;

impl Service for MarketService {
    type Cli = ();
}

impl MarketService {
    pub async fn gsb<Context>(_: &Context) -> anyhow::Result<()> {
        let _ = bus::bind(market::BUS_ID, |get: market::GetAgreement| async move {
            Ok(get_agreement(&get).await?)
        });

        tmp_send_keys()
            .await
            .unwrap_or_else(|e| log::error!("app-key export error: {}", e));

        Ok(())
    }
}

async fn get_agreement(get: &market::GetAgreement) -> Result<market::Agreement, Error> {
    let market_api: MarketProviderApi = WebClient::builder().build()?.interface()?;

    Ok(market_api.get_agreement(&get.agreement_id).await?)
}

async fn tmp_send_keys() -> anyhow::Result<()> {
    let (ids, _n) = bus::service(appkey::BUS_ID)
        .send(appkey::List {
            identity: None,
            page: 1,
            per_page: 10,
        })
        .await??;

    let ids: Vec<serde_json::Value> = ids
        .into_iter()
        .map(|k: appkey::AppKey| serde_json::json! {{"key": k.key, "nodeId": k.identity}})
        .collect();
    log::debug!("exporting all app-keys: {:#?}", &ids);

    let mut url =
        MarketProviderApi::rebase_service_url(Rc::new(Url::parse("http://127.0.0.1:5001")?))?
            .as_ref()
            .clone();
    url.set_path("admin/import-key");
    log::debug!("posting to: {:?}", url);

    let request: awc::FrozenClientRequest = awc::Client::new()
        .post(url.to_string())
        .freeze()
        .map_err(|e| anyhow!("Failed to build frozen request. Error: {}", e.to_string()))?;

    for count in 0..KEY_EXPORT_RETRY_COUNT {
        match request.send_json(&ids).await {
            Ok(mut response) => {
                let parsed_response: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| anyhow!("Failed to parse key export response. Error: {}", e))?;

                log::info!(
                    "Key export successful, exported keys count: {}",
                    parsed_response
                );
                break;
            }
            Err(e) => {
                if count == KEY_EXPORT_RETRY_COUNT - 1 {
                    log::error!("Key export failed, no retries left. Error: {}", e);
                } else {
                    log::debug!(
                        "Key export failed, retrying in: {} seconds. Retries left: {}",
                        KEY_EXPORT_RETRY_DELAY.as_secs(),
                        KEY_EXPORT_RETRY_COUNT - (count + 1)
                    );
                    delay_for(KEY_EXPORT_RETRY_DELAY).await;
                }
            }
        }
    }

    Ok(())
}
