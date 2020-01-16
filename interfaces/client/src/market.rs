//! Market API part of the Yagna API
use std::sync::Arc;

use crate::{web::WebClientBuilder, Result};
pub use ya_service_api::constants::MARKET_API;

mod provider;
pub use provider::ProviderApi;
mod requestor;
pub use requestor::RequestorApi;

/// Client for the Market API. Supports both sides: Provider and Requestor.
pub struct ApiClient {
    provider: ProviderApi,
    requestor: RequestorApi,
}

impl ApiClient {
    /// Constructs new `ApiClient`.
    pub fn new(client: WebClientBuilder) -> Result<Self> {
        let client = Arc::new(client.api_root(MARKET_API).build()?);

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
