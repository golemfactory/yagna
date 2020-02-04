use std::convert::TryInto;
use ya_client::{market::MarketProviderApi, web::WebClient};
use ya_core_model::market as model;
use ya_core_model::{appkey, market};
use ya_model::market::Agreement;
use ya_persistence::executor::DbExecutor;
use ya_service_api::constants::NET_SERVICE_ID;
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use crate::{dao, error::Error};
use ya_core_model::market::RpcMessageError;

pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;

pub fn activate(db: &DbExecutor) {
    let _ = bus::bind(market::BUS_ID, |get: market::GetAgreement| async move {
        let market_api: MarketProviderApi = WebClient::builder()
            .build()
            .map_err(|e| market::RpcMessageError::Market(e.to_string()))?
            .interface()
            .map_err(|e| market::RpcMessageError::Market(e.to_string()))?;
        let agreement = market_api
            .get_agreement(&get.agreement_id)
            .await
            .map_err(|e| market::RpcMessageError::Market(e.to_string()))?;
        Ok(agreement)
    });
}
