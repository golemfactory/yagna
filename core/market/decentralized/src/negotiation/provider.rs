use std::sync::Arc;

use crate::{db::models::Offer as ModelOffer, SubscriptionId};
use ya_persistence::executor::DbExecutor;

use super::errors::{NegotiationError, NegotiationInitError};
use crate::protocol::negotiation::messages::{
    AgreementCancelled, AgreementReceived, InitialProposalReceived, ProposalReceived,
    ProposalRejected,
};
use crate::protocol::negotiation::provider::NegotiationApi;

/// Provider part of negotiation logic.
/// TODO: Too long name.
pub struct ProviderNegotiationEngine {
    db: DbExecutor,
    api: NegotiationApi,
}

impl ProviderNegotiationEngine {
    pub fn new(db: DbExecutor) -> Result<Arc<ProviderNegotiationEngine>, NegotiationInitError> {
        let api = NegotiationApi::new(
            move |_caller: String, msg: InitialProposalReceived| async move { unimplemented!() },
            move |_caller: String, msg: ProposalReceived| async move { unimplemented!() },
            move |caller: String, msg: ProposalRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementReceived| async move { unimplemented!() },
            move |caller: String, msg: AgreementCancelled| async move { unimplemented!() },
        );

        Ok(Arc::new(ProviderNegotiationEngine { api, db }))
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
