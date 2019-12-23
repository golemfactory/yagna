//! Market API part of the Yagna API
use crate::{web::WebClientBuilder, Result};
use std::sync::Arc;

mod provider;
pub use provider::ProviderApi;
mod requestor;
pub use requestor::RequestorApi;

/// Client for the Market API. Supports both sides: Provider and Requestor.
pub struct ApiClient {
    provider: ProviderApi,
    requestor: RequestorApi,
}

pub const API_ROOT: &str = "/market-api/v1/";

impl ApiClient {
    /// Constructs new `ApiClient`.
    pub fn new(client: WebClientBuilder) -> Result<Self> {
        let client = Arc::new(client.api_root(API_ROOT).build()?);

        Ok(ApiClient {
            provider: ProviderApi::new(&client),
            requestor: RequestorApi::new(&client),
        })
    }

    /// Provider's part of the Market API.
    pub fn provider(&self) -> &ProviderApi {
        &self.provider
    }

    /// Requestor's part of the Market API.
    pub fn requestor(&self) -> &RequestorApi {
        &self.requestor
    }
}
