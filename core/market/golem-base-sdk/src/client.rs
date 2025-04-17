use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use std::sync::Arc;
use url::Url;

/// Client for interacting with Golem Base node
#[derive(Clone)]
pub struct GolemBaseClient {
    /// The underlying provider for making RPC calls
    provider: Arc<Box<DynProvider>>,
}

impl GolemBaseClient {
    /// Creates a new client connected to the specified endpoint
    pub fn new(endpoint: Url) -> Self {
        let provider = ProviderBuilder::new().on_http(endpoint).erased();

        Self {
            provider: Arc::new(Box::new(provider)),
        }
    }

    /// Gets the chain ID of the connected node
    pub async fn get_chain_id(&self) -> anyhow::Result<u64> {
        Ok(self.provider.get_chain_id().await?)
    }
}
