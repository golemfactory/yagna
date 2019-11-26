use std::rc::Rc;
use std::sync::Arc;

use futures::Future;
use url::Url;

use error::Error;

/// Connection to an API endpoint.
#[derive(Clone, Debug)]
pub struct ApiConnection {
    api_url: Arc<Url>
}

impl Default for ApiConnection {
    fn default() -> Self {
        match env::var("API_ADDR") {
            Ok(addr) => ApiConnection::from_addr(addr).unwrap(),
            Err(_) => ApiConnection::from_addr("127.0.0.1:5001").unwrap(),
        }
    }
}

impl ApiConnection {
    /// creates an API connection to a given address
    pub fn from_addr<T: Into<String>>(addr: T) -> Result<ApiConnection, Error> {
        Url::parse(&format!("http://{}/", addr.into()))
            .map_err(Error::InvalidAddress)
            .map(|url| ApiConnection { api_url: Arc::new(url) })
    }

    pub fn provider_api(&self) -> &::apis::ProviderApi {
        self.provider_api.as_ref()
    }
}
