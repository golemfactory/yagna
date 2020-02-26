use std::rc::Rc;
use url::Url;

use crate::Error;
use ya_client::{
    market::MarketProviderApi,
    web::{WebClient, WebInterface},
};
use ya_core_model::{appkey, market};
use ya_service_api_interfaces::Service;
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;

pub struct MarketService;

impl Service for MarketService {
    type Cli = ();
}

impl MarketService {
    pub async fn gsb<Context>(_: &Context) -> anyhow::Result<()> {
        let _ = bus::bind(market::BUS_ID, |get: market::GetAgreement| async move {
            let market_api: MarketProviderApi = WebClient::builder()
                .build()
                .map_err(Error::from)?
                .interface()
                .map_err(Error::from)?;
            let agreement = market_api
                .get_agreement(&get.agreement_id)
                .await
                .map_err(Error::from)?;
            Ok(agreement)
        });

        tmp_send_keys()
            .await
            .unwrap_or_else(|e| log::error!("app-key export error: {}", e));

        Ok(())
    }
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

    let resp: serde_json::Value = awc::Client::new()
        .post(url.to_string())
        .send_json(&ids)
        .await
        .map_err(|e| anyhow::Error::msg(e.to_string()))?
        .json()
        .await
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    log::info!("done. number of keys exported: {}", resp);

    Ok(())
}
