use std::sync::Arc;

use ya_persistence::executor::DbExecutor;
use ya_client::model::market::Offer;

use super::errors::{NegotiationError, NegotiationInitError};

/// Provider part of negotiation logic.
/// TODO: Too long name.
pub struct ProviderNegotiationEngine {
    db: DbExecutor,
}

impl ProviderNegotiationEngine {
    pub fn new(db: DbExecutor) -> Result<Arc<ProviderNegotiationEngine>, NegotiationInitError> {
        Ok(Arc::new(ProviderNegotiationEngine { db }))
    }

    pub async fn bind_gsb(&self, prefix: String) -> Result<(), NegotiationInitError> {
        Ok(())
    }

    pub async fn subscribe_offer(&self, offer: &Offer) -> Result<(), NegotiationError> {
        unimplemented!();
    }

    pub async fn unsubscribe_offer(&self, subscription_id: String) -> Result<(), NegotiationError> {
        unimplemented!();
    }
}
