use std::env;

use crate::Error;

/// API configuration.
#[derive(Clone, Debug)]
pub struct ApiConfiguration {
    api_url: String,
    // TODO: access_token: Option<JWT>
}

impl Default for ApiConfiguration {
    fn default() -> Self {
        match env::var("MARKET_API_ADDR") {
            Ok(addr) => ApiConfiguration::from_addr(addr).unwrap(),
            Err(_) => ApiConfiguration::from_addr("127.0.0.1:5001/market-api/v1").unwrap(),
        }
    }
}

impl ApiConfiguration {
    /// creates an API connection to a given address
    pub fn from_addr<T: Into<String>>(addr: T) -> Result<ApiConfiguration, Error> {
        format!("http://{}/", addr.into())
            .parse()
            .map_err(Error::InvalidAddress)
            .map(|api_url| ApiConfiguration { api_url })
    }

    pub fn api_endpoint<T: Into<String>>(&self, input: T) -> String {
        format!("{}{}", self.api_url, input.into())
    }
}
