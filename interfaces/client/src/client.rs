use std::{env, rc::Rc, sync::Arc};

use futures::Future;
use url::Url;

use crate::{error::Error, provider_api::ProviderApi};

/// Client for an API endpoint.
#[derive(Clone, Debug)]
pub struct ApiClient {
    api_url: Arc<Url>,
}

impl Default for ApiClient {
    fn default() -> Self {
        match env::var("API_ADDR") {
            Ok(addr) => ApiClient::from_addr(addr).unwrap(),
            Err(_) => ApiClient::from_addr("127.0.0.1:5001").unwrap(),
        }
    }
}

impl ApiClient {
    /// creates an API connection to a given address
    pub fn from_addr<T: Into<String>>(addr: T) -> Result<ApiClient, Error> {
        Url::parse(&format!("http://{}/", addr.into()))
            .map_err(Error::InvalidAddress)
            .map(|url| ApiClient {
                api_url: Arc::new(url),
            })
    }

    pub fn provider_api(&self) -> &ProviderApi {
        self.provider_api.as_ref()
    }
}
