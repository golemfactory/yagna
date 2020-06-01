use std::sync::Arc;

use crate::db::models::Offer as ModelOffer;
use ya_persistence::executor::DbExecutor;

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

    pub async fn bind_gsb(
        &self,
        public_prefix: String,
        private: String,
    ) -> Result<(), NegotiationInitError> {
        Ok(())
    }

    pub async fn subscribe_offer(&self, offer: &ModelOffer) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }

    pub async fn unsubscribe_offer(
        &self,
        subscription_id: &String,
    ) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }
}
