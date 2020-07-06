use std::str::FromStr;
use std::sync::Arc;

use ya_client::model::NodeId;
use ya_persistence::executor::DbExecutor;

use super::errors::{NegotiationError, NegotiationInitError};
use crate::db::dao::{EventsDao, ProposalDao};
use crate::db::models::OwnerType;
use crate::matcher::OfferError;
use crate::matcher::SubscriptionStore;
use crate::negotiation::notifier::EventNotifier;
use crate::protocol::negotiation::errors::{CounterProposalError, RemoteProposalError};
use crate::protocol::negotiation::messages::{
    AgreementCancelled, AgreementReceived, InitialProposalReceived, ProposalReceived,
    ProposalRejected,
};
use crate::protocol::negotiation::provider::NegotiationApi;
use crate::{db::models::Offer as ModelOffer, SubscriptionId};

/// Provider part of negotiation logic.
#[derive(Clone)]
pub struct ProviderBroker {
    db: DbExecutor,
    store: SubscriptionStore,
    api: NegotiationApi,
    notifier: EventNotifier,
}

impl ProviderBroker {
    pub fn new(
        db: DbExecutor,
        store: SubscriptionStore,
    ) -> Result<Arc<ProviderBroker>, NegotiationInitError> {
        let notifier = EventNotifier::new();

        let db1 = db.clone();
        let notifier1 = notifier.clone();
        let store1 = store.clone();

        let api = NegotiationApi::new(
            move |caller: String, msg: InitialProposalReceived| {
                on_initial_proposal(db1.clone(), store1.clone(), notifier1.clone(), caller, msg)
            },
            move |_caller: String, msg: ProposalReceived| async move { unimplemented!() },
            move |caller: String, msg: ProposalRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementReceived| async move { unimplemented!() },
            move |caller: String, msg: AgreementCancelled| async move { unimplemented!() },
        );

        Ok(Arc::new(ProviderBroker {
            api,
            store,
            db,
            notifier,
        }))
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), NegotiationInitError> {
        Ok(self.api.bind_gsb(public_prefix, private_prefix).await?)
    }

    pub async fn subscribe_offer(&self, offer: &ModelOffer) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }

    pub async fn unsubscribe_offer(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Result<(), NegotiationError> {
        self.notifier.stop_notifying(subscription_id).await;
        Ok(())
    }
}

async fn on_initial_proposal(
    db: DbExecutor,
    store: SubscriptionStore,
    notifier: EventNotifier,
    caller: String,
    msg: InitialProposalReceived,
) -> Result<(), CounterProposalError> {
    // Check subscription.
    let offer = match store.get_offer(&msg.offer_id).await {
        Err(e) => match e {
            OfferError::AlreadyUnsubscribed(id) => Err(RemoteProposalError::Unsubscribed(id))?,
            OfferError::Expired(id) => Err(RemoteProposalError::Expired(id))?,
            _ => Err(RemoteProposalError::Unexpected(e.to_string()))?,
        },
        Ok(offer) => offer,
    };

    // Add proposal to database together with Negotiation record.
    let owner_id =
        NodeId::from_str(&caller).map_err(|e| RemoteProposalError::Unexpected(e.to_string()))?;
    let demand = msg.into_demand(owner_id);
    let proposal = db
        .as_dao::<ProposalDao>()
        .new_initial_proposal(demand, offer)
        .await
        .map_err(|e| RemoteProposalError::Unexpected(e.to_string()))?;

    // Create Proposal Event and add it to queue (database).
    let subscription_id = proposal.negotiation.subscription_id.clone();
    db.as_dao::<EventsDao>()
        .add_proposal_event(proposal, OwnerType::Provider)
        .await
        .map_err(|e| RemoteProposalError::Unexpected(e.to_string()))?;

    // Send channel message to wake all query_events waiting for proposals.
    notifier.notify(&subscription_id).await;
    Ok(())
}
