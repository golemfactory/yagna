use std::sync::Arc;

pub mod provider;
pub mod requestor;

mod configuration;
pub use configuration::ApiConfiguration;

pub struct ApiClient {
    provider: provider::ProviderApi,
    requestor: requestor::RequestorApi,
}

impl ApiClient {
    pub fn new(configuration: ApiConfiguration) -> ApiClient {
        let arc = Arc::new(configuration);

        ApiClient {
            provider: provider::ProviderApi::new(arc.clone()),
            requestor: requestor::RequestorApi::new(arc.clone()),
        }
    }

    pub fn provider(&self) -> &provider::ProviderApi {
        &self.provider
    }

    pub fn requestor(&self) -> &requestor::RequestorApi {
        &self.requestor
    }
}
