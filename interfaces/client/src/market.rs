use std::sync::Arc;
use crate::{Result, web::WebClientBuilder};

pub mod provider;
pub mod requestor;

pub struct ApiClient {
    provider: provider::ProviderApi,
    requestor: requestor::RequestorApi,
}

pub const API_ROOT: &str = "/market-api/v1";

impl ApiClient {
    pub fn new(client: WebClientBuilder) -> Result<Self> {
        let client = Arc::new(client.api_root(API_ROOT).build()?);

        Ok(ApiClient {
            provider: provider::ProviderApi::new(client.clone()),
            requestor: requestor::RequestorApi::new(client.clone()),
        })
    }

    pub fn provider(&self) -> &provider::ProviderApi {
        &self.provider
    }

    pub fn requestor(&self) -> &requestor::RequestorApi {
        &self.requestor
    }
}
