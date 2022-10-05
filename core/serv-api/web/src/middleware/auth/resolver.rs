use actix_web::error::ErrorInternalServerError;
use actix_web::Error;
use futures::{Future, TryFutureExt};
use std::pin::Pin;
use ya_core_model::appkey::{self, AppKey, Get};
use ya_service_api_cache::ValueResolver;
use ya_service_bus::actix_rpc;

#[derive(Default)]
pub struct AppKeyResolver;

impl ValueResolver for AppKeyResolver {
    type Key = String;
    type Value = AppKey;
    type Error = Error;

    fn resolve<'a>(
        &self,
        key: &Self::Key,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Self::Value>, Self::Error>> + 'a>> {
        let key = key.clone();
        Box::pin(async move {
            let resp = actix_rpc::service(appkey::BUS_ID)
                .send(Get::with_key(key))
                .map_err(|e| ErrorInternalServerError(format!("{}", e)))
                .await?;
            Ok(resp.ok())
        })
    }
}
