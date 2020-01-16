use actix_web::error::ErrorInternalServerError;
use actix_web::Error;
use futures::{Future, TryFutureExt};
use std::pin::Pin;
use ya_core_model::appkey::{AppKey, Get, APP_KEY_SERVICE_ID};
use ya_service_api_cache::ValueResolver;
use ya_service_bus::actix_rpc;

pub struct AppKeyResolver;

impl Default for AppKeyResolver {
    fn default() -> Self {
        AppKeyResolver {}
    }
}

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
            let resp = actix_rpc::service(APP_KEY_SERVICE_ID)
                .send(Get { key })
                .map_err(|e| ErrorInternalServerError(format!("{}", e)))
                .await?;
            Ok(resp.ok())
        })
    }
}
