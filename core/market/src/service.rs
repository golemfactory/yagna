use ya_client::{market::MarketProviderApi, web::WebClient};

use ya_core_model::market;

use ya_persistence::executor::DbExecutor;

use ya_service_bus::{typed as bus, RpcMessage};

pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;

pub fn activate(_db: &DbExecutor) {
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
