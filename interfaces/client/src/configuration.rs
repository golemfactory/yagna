//! Configuration tools
use std::env;

use crate::Result;
use url::Url;

const API_HOST_PORT: &str = "localhost:5001";

/// API configuration.
#[derive(Clone, Debug)]
pub struct ApiConfiguration {
    api_url: Url,
}

impl Default for ApiConfiguration {
    fn default() -> Self {
        ApiConfiguration::from(None, None).unwrap()
    }
}

impl ApiConfiguration {
    pub fn from(host_port: Option<String>, path: Option<String>) -> Result<ApiConfiguration> {
        Url::parse(&format!(
            "http://{host_port}{path}",
            host_port = host_port
                .or_else(|| env::var("API_ADDR").ok())
                .unwrap_or(API_HOST_PORT.into()),
            path = path.unwrap_or("".into())
        ))
        .map(|api_url| ApiConfiguration { api_url })
        .map_err(From::from)
    }

    pub fn endpoint_url<T: Into<String>>(&self, endpoint: T) -> Url {
        self.api_url.join(&endpoint.into()).unwrap()
    }
}
