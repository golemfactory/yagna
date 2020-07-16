// TODO: This is only temporary
#![allow(dead_code)]
use chrono::Utc;
use futures::stream::StreamExt;
use std::str::FromStr;

use ya_client::model::{
    market::{event::ProviderEvent, Proposal as ClientProposal},
    NodeId,
};
use ya_persistence::executor::DbExecutor;

use super::error::{NegotiationError, NegotiationInitError};
use crate::db::dao::{AgreementDao, EventsDao, ProposalDao};
use crate::db::model::{AgreementId, AgreementState, Offer as ModelOffer, SubscriptionId};
use crate::db::model::{OwnerType, Proposal, ProposalId};
use crate::matcher::{error::QueryOfferError, store::SubscriptionStore};
use crate::negotiation::common::CommonBroker;
use crate::negotiation::error::{AgreementError, ProposalError, QueryEventsError};
use crate::negotiation::notifier::EventNotifier;
use crate::protocol::negotiation::error::{
    ConfirmAgreementError, CounterProposalError, RemoteProposalError,
};
use crate::protocol::negotiation::messages::{
    AgreementCancelled, AgreementReceived, InitialProposalReceived, ProposalReceived,
    ProposalRejected,
};
use crate::protocol::negotiation::provider::NegotiationApi;

/// Provider part of negotiation logic.
#[derive(Clone)]
pub struct ProviderBroker {
    common: CommonBroker,
    api: NegotiationApi,
}

impl ProviderBroker {
    pub fn new(
        db: DbExecutor,
        store: SubscriptionStore,
    ) -> Result<ProviderBroker, NegotiationInitError> {
        let notifier = EventNotifier::new();
        let broker = CommonBroker {
            store,
            db,
            notifier,
        };

        let broker1 = broker.clone();
        let broker2 = broker.clone();
        let broker3 = broker.clone();

        let api = NegotiationApi::new(
            move |caller: String, msg: InitialProposalReceived| {
                on_initial_proposal(broker1.clone(), caller, msg)
            },
            move |caller: String, msg: ProposalReceived| {
                broker2
                    .clone()
                    .on_proposal_received(caller, msg, OwnerType::Provider)
            },
            move |_caller: String, _msg: ProposalRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementReceived| {
                on_agreement_received(broker3.clone(), caller, msg)
            },
            move |_caller: String, _msg: AgreementCancelled| async move { unimplemented!() },
        );

        Ok(ProviderBroker {
            api,
            common: broker,
        })
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), NegotiationInitError> {
        Ok(self.api.bind_gsb(public_prefix, private_prefix).await?)
    }

    pub async fn subscribe_offer(&self, _offer: &ModelOffer) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }

    pub async fn unsubscribe_offer(
        &self,
        offer_id: &SubscriptionId,
    ) -> Result<(), NegotiationError> {
        self.common.notifier.stop_notifying(offer_id).await;
        Ok(())
    }

    pub async fn counter_proposal(
        &self,
        subscription_id: &SubscriptionId,
        prev_proposal_id: &ProposalId,
        proposal: &ClientProposal,
    ) -> Result<ProposalId, ProposalError> {
        let (new_proposal, _) = self
            .common
            .counter_proposal(subscription_id, prev_proposal_id, proposal)
            .await?;

        let proposal_id = new_proposal.body.id.clone();
        self.api
            .counter_proposal(new_proposal)
            .await
            .map_err(|e| ProposalError::FailedSendProposal(prev_proposal_id.clone(), e))?;

        Ok(proposal_id)
    }

    pub async fn query_events(
        &self,
        offer_id: &SubscriptionId,
        timeout: f32,
        max_events: Option<i32>,
    ) -> Result<Vec<ProviderEvent>, QueryEventsError> {
        let events = self
            .common
            .query_events(offer_id, timeout, max_events, OwnerType::Provider)
            .await?;

        // Map model events to client RequestorEvent.
        Ok(futures::stream::iter(events)
            .then(|event| event.into_client_provider_event(&self.common.db))
            .inspect(|result| {
                if let Err(error) = result {
                    log::warn!("Error converting event to client type: {}", error);
                }
            })
            .filter_map(|event| async move { event.ok() })
            .collect::<Vec<ProviderEvent>>()
            .await)
    }

    pub async fn approve_agreement(
        &self,
        agreement_id: &AgreementId,
        timeout: f32,
    ) -> Result<(), AgreementError> {
        let dao = self.common.db.as_dao::<AgreementDao>();

        let agreement = match dao
            .select(agreement_id, Utc::now().naive_utc())
            .await
            .map_err(|e| AgreementError::Get(agreement_id.clone(), e))?
        {
            None => return Err(AgreementError::NotFound(agreement_id.clone())),
            Some(agreement) => agreement,
        };

        Err(match agreement.state {
            AgreementState::Proposal => AgreementError::Proposal(agreement.id),
            AgreementState::Pending => {
                // TODO : possible race condition here ISSUE#430
                // 1. this state check should be also `db.update_state`
                // 2. `db.update_state` must be invoked after successful propose_agreement
                self.api.approve_agreement(agreement).await?;
                dao.update_state(agreement_id, AgreementState::Approved)
                    .await
                    .map_err(|e| AgreementError::Get(agreement_id.clone(), e))?;
                return Ok(());
            }
            AgreementState::Cancelled => AgreementError::Cancelled(agreement.id),
            AgreementState::Rejected => AgreementError::Rejected(agreement.id),
            AgreementState::Approved => AgreementError::Approved(agreement.id),
            AgreementState::Expired => AgreementError::Expired(agreement.id),
            AgreementState::Terminated => AgreementError::Terminated(agreement.id),
        })
    }
}

async fn on_initial_proposal(
    broker: CommonBroker,
    caller: String,
    msg: InitialProposalReceived,
) -> Result<(), CounterProposalError> {
    let db = broker.db;
    let store = broker.store;
    let notifier = broker.notifier;

    // Check subscription.
    let offer = match store.get_offer(&msg.offer_id).await {
        Err(e) => match e {
            QueryOfferError::Unsubscribed(id) => Err(RemoteProposalError::Unsubscribed(id))?,
            QueryOfferError::Expired(id) => Err(RemoteProposalError::Expired(id))?,
            _ => Err(RemoteProposalError::Unexpected(e.to_string()))?,
        },
        Ok(offer) => offer,
    };

    // Add proposal to database together with Negotiation record.
    let owner_id =
        NodeId::from_str(&caller).map_err(|e| RemoteProposalError::Unexpected(e.to_string()))?;
    let proposal = Proposal::new_provider(&msg.demand_id, owner_id, msg.proposal, offer);
    let proposal = db
        .as_dao::<ProposalDao>()
        .save_initial_proposal(proposal)
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

async fn on_agreement_received(
    broker: CommonBroker,
    _caller: String,
    mut msg: AgreementReceived,
) -> Result<(), ConfirmAgreementError> {
    let id = msg.agreement.id.clone();
    let subscription_id = msg.agreement.offer_id.clone();
    broker
        .db
        .as_dao::<AgreementDao>()
        .save(msg.agreement.clone())
        .await
        .map_err(|e| ConfirmAgreementError::Saving(e.to_string(), id.clone()))?;

    // TODO: we should build new agreement here from local Proposal and signatures
    msg.agreement.state = AgreementState::Pending;

    broker
        .db
        .as_dao::<EventsDao>()
        .add_agreement_event(msg.agreement)
        .await
        .map_err(|e| ConfirmAgreementError::Saving(e.to_string(), id.clone()))?;

    // Send channel message to wake all query_events waiting for proposals.
    broker.notifier.notify(&subscription_id).await;

    Ok(())
}
