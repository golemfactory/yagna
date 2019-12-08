use std::sync::Arc;

pub mod provider;
pub mod requestor;

mod configuration;
pub use configuration::ApiConfiguration;

pub struct ApiClient {
    provider: Box<dyn provider::ProviderApi>,
//    requestor: Box<dyn requestor::RequestorApi>,
}

impl ApiClient {
    pub fn new(configuration: ApiConfiguration) -> ApiClient {
        let arc = Arc::new(configuration);

        ApiClient {
            provider: Box::new(provider::ProviderApiClient::new(arc.clone())),
//            requestor: Box::new(requestor::RequestorApiClient::new(arc.clone())),
        }
    }

    pub fn provider(&self) -> &dyn provider::ProviderApi{
        self.provider.as_ref()
    }

//    pub fn requestor(&self) -> &dyn requestor::RequestorApi{
//        self.requestor.as_ref()
//    }

}



