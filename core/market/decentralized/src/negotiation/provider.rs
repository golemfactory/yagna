use std::sync::Arc;

use crate::{db::models::Offer as ModelOffer, SubscriptionId};
use ya_persistence::executor::DbExecutor;

use super::errors::{NegotiationError, NegotiationInitError};

/// Provider part of negotiation logic.
pub struct ProviderBroker {
    db: DbExecutor,
}

impl ProviderBroker {
    pub fn new(db: DbExecutor) -> Result<Arc<ProviderBroker>, NegotiationInitError> {
        Ok(Arc::new(ProviderBroker { db }))
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), NegotiationInitError> {
        Ok(())
    }

    pub async fn subscribe_offer(&self, offer: &ModelOffer) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }

    pub async fn unsubscribe_offer(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }
}
