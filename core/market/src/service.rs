use anyhow::anyhow;
use std::{convert::TryInto, rc::Rc, time::Duration};
use url::Url;

use ya_client::{
    market::MarketProviderApi,
    web::{WebClient, WebInterface},
};
use ya_core_model::{appkey, market};
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use crate::{dao::AgreementDao, Error};

pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;

pub struct MarketService;

impl Service for MarketService {
    type Cli = ();
}

impl MarketService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> anyhow::Result<()> {
        let db = ctx.component();
        crate::dao::init(&db)?;
        let client = WebClient::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        let _ = bus::bind(market::BUS_ID, move |get: market::GetAgreement| {
            let market_api: MarketProviderApi = client.interface(None).unwrap();
            let db = db.clone();

            async move {
                let dao = db.as_dao::<AgreementDao>();
                if let Ok(agreement) = dao.get(get.agreement_id.clone()).await {
                    log::debug!("got agreement from db: {}", agreement.natural_id);
                    return Ok(agreement.try_into().map_err(Error::from)?);
                }

                log::debug!("fetching agreement [{}] via REST", get.agreement_id);
                let agreement = market_api
                    .get_agreement(&get.agreement_id)
                    .await
                    .map_err(|e| market::RpcMessageError::Service(e.to_string()))?;

                log::debug!("inserting agreement: {}", agreement.agreement_id);
                log::trace!("inserting agreement: {:#?}", agreement);
                dao.create(agreement.clone().try_into().map_err(Error::from)?)
                    .await
                    .map_err(Error::from)?;

                Ok(agreement)
            }
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
        .map_err(|e| anyhow!("posting to: {:?} error: {}", url, e.to_string()))?
        .json()
        .await
        .map_err(|e| anyhow!("key export response decoding error: {}", e.to_string()))?;
    log::info!("done. number of keys exported: {}", resp);

    Ok(())
}
