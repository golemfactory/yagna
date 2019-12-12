//! Market API part of the Yagna API
use crate::{web::WebClientBuilder, Result};
use std::sync::Arc;

pub mod provider;
pub mod requestor;

/// Client for the Market API. Supports both sides: Provider and Requestor.
pub struct ApiClient {
    provider: provider::ProviderApi,
    requestor: requestor::RequestorApi,
}

pub const API_ROOT: &str = "/market-api/v1/";

impl ApiClient {
    /// Constructs new `ApiClient`.
    pub fn new(client: WebClientBuilder) -> Result<Self> {
        let client = Arc::new(client.api_root(API_ROOT).build()?);

        Ok(ApiClient {
            provider: provider::ProviderApi::new(client.clone()),
            requestor: requestor::RequestorApi::new(client.clone()),
        })
    }

    /// Provider's part of the Market API.
    pub fn provider(&self) -> &provider::ProviderApi {
        &self.provider
    }

    /// Requestor's part of the Market API.
    pub fn requestor(&self) -> &requestor::RequestorApi {
        &self.requestor
    }
}
