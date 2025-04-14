use alloy::providers::{Provider, ProviderBuilder};
use std::sync::Arc;
use url::Url;

/// Client for interacting with Golem Base node
#[derive(Clone)]
pub struct GolemBaseClient {
    provider: Arc<Box<dyn Provider>>,
}

impl GolemBaseClient {
    /// Creates a new client connected to the specified endpoint
    pub fn new(endpoint: Url) -> Self {
        let provider = ProviderBuilder::new().on_http(endpoint).erased();

        Self {
            provider: Arc::new(Box::new(provider)),
        }
    }
}
