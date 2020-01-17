//! Configuration tools
use url::Url;

use ya_service_api::constants::YAGNA_HTTP_ADDR_STR;

use crate::Result;

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
            host_port = host_port.unwrap_or(YAGNA_HTTP_ADDR_STR.to_string()),
            path = path.unwrap_or("".into())
        ))
        .map(|api_url| ApiConfiguration { api_url })
        .map_err(From::from)
    }

    pub fn endpoint_url<T: Into<String>>(&self, endpoint: T) -> Url {
        self.api_url.join(&endpoint.into()).unwrap()
    }
}
