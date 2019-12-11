use std::env;

use crate::Error;

const API_HOST_PORT: &str = "localhost:5001";

/// API configuration.
#[derive(Clone, Debug)]
pub struct ApiConfiguration {
    api_uri: String,
}

impl Default for ApiConfiguration {
    fn default() -> Self {
        ApiConfiguration::from(None, None).unwrap()
    }
}

impl ApiConfiguration {
    /// creates an API connection to a given address
    pub fn from_addr<T: Into<String>>(addr: T) -> Result<ApiConfiguration, Error> {
        format!("http://{}", addr.into())
            .parse()
            .map_err(Error::InvalidAddress)
            .map(|api_uri| ApiConfiguration { api_uri })
    }

    pub fn from(host_port: Option<String>, api_root: Option<String>)
        -> Result<ApiConfiguration, Error> {

        ApiConfiguration::from_addr(format!(
            "{}{}",
            host_port.or_else(|| env::var("API_ADDR").ok()).unwrap_or(API_HOST_PORT.into()),
            api_root.unwrap_or("".into())
        ))
    }


    pub fn api_endpoint<T: Into<String>>(&self, input: T) -> String {
        format!("{}{}", self.api_uri, input.into())
    }
}
