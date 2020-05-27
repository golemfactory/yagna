use std::{convert::TryInto, time::Duration};

use ya_client::{market::MarketProviderApi, web::WebClient};
use ya_core_model::market;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};
use ya_service_bus::{typed as bus, RpcMessage};

use crate::{api, dao::AgreementDao, Error};

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
            let market_api: MarketProviderApi = client
                .interface_at(Some(crate::api::CENTRAL_MARKET_URL.clone()))
                .unwrap();
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

        Ok(())
    }

    pub fn rest<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> actix_web::Scope {
        api::web_scope(&ctx.component())
    }
}
