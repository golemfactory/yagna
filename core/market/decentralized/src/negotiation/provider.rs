use std::sync::Arc;

use crate::{db::models::Offer as ModelOffer, SubscriptionId};
use ya_persistence::executor::DbExecutor;

use super::errors::{NegotiationError, NegotiationInitError};
use crate::protocol::negotiation::errors::ProposalError;
use crate::protocol::negotiation::messages::{
    AgreementCancelled, AgreementReceived, InitialProposalReceived, ProposalReceived,
    ProposalRejected,
};
use crate::protocol::negotiation::provider::NegotiationApi;

/// Provider part of negotiation logic.
#[derive(Clone)]
pub struct ProviderBroker {
    db: DbExecutor,
    api: NegotiationApi,
}

impl ProviderBroker {
    pub fn new(db: DbExecutor) -> Result<Arc<ProviderBroker>, NegotiationInitError> {
        let db1 = db.clone();
        let api = NegotiationApi::new(
            move |caller: String, msg: InitialProposalReceived| {
                let db = db1.clone();
                on_initial_proposal(db, caller, msg)
            },
            move |_caller: String, msg: ProposalReceived| async move { unimplemented!() },
            move |caller: String, msg: ProposalRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementReceived| async move { unimplemented!() },
            move |caller: String, msg: AgreementCancelled| async move { unimplemented!() },
        );

        Ok(Arc::new(ProviderBroker { api, db }))
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), NegotiationInitError> {
        self.api.bind_gsb(public_prefix, private_prefix).await?;
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

async fn on_initial_proposal(
    db: DbExecutor,
    caller: String,
    msg: InitialProposalReceived,
) -> Result<(), ProposalError> {
    Ok(())
}
