use chrono::{DateTime, Utc};
use futures::stream::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

use super::errors::{NegotiationError, NegotiationInitError, QueryEventsError};
use super::EventNotifier;
use crate::db::dao::AgreementDao;
use crate::db::dao::{EventsDao, ProposalDao};
use crate::db::models::OwnerType;
use crate::db::models::{
    Agreement, AgreementId, Demand as ModelDemand, Proposal, ProposalId, SubscriptionId,
};
use crate::db::DbResult;
use crate::matcher::DraftProposal;
use crate::matcher::SubscriptionStore;

use ya_client::model::market::event::RequestorEvent;
use ya_client::model::market::proposal::Proposal as ClientProposal;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::negotiation::common::CommonBroker;
use crate::negotiation::errors::AgreementError;
use crate::negotiation::ProposalError;
use crate::protocol::negotiation::messages::{
    AgreementApproved, AgreementRejected, ProposalReceived, ProposalRejected,
};
use crate::protocol::negotiation::requestor::NegotiationApi;

/// Requestor part of negotiation logic.
pub struct RequestorBroker {
    common: CommonBroker,
    api: NegotiationApi,
}

impl RequestorBroker {
    pub fn new(
        db: DbExecutor,
        store: SubscriptionStore,
        proposal_receiver: UnboundedReceiver<DraftProposal>,
    ) -> Result<Arc<RequestorBroker>, NegotiationInitError> {
        let api = NegotiationApi::new(
            move |_caller: String, msg: ProposalReceived| async move { unimplemented!() },
            move |_caller: String, msg: ProposalRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementApproved| async move { unimplemented!() },
            move |caller: String, msg: AgreementRejected| async move { unimplemented!() },
        );

        let notifier = EventNotifier::new();
        let broker = CommonBroker {
            store,
            db: db.clone(),
            notifier: notifier.clone(),
        };

        let engine = RequestorBroker {
            api,
            common: broker,
        };

        tokio::spawn(proposal_receiver_thread(db, proposal_receiver, notifier));
        Ok(Arc::new(engine))
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), NegotiationInitError> {
        self.api.bind_gsb(public_prefix, private_prefix).await?;
        Ok(())
    }

    pub async fn subscribe_demand(&self, demand: &ModelDemand) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }

    pub async fn unsubscribe_demand(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Result<(), NegotiationError> {
        self.common.notifier.stop_notifying(subscription_id).await;

        // We can ignore error, if removing events failed, because they will be never
        // queried again and don't collide with other subscriptions.
        let _ = self
            .db()
            .as_dao::<EventsDao>()
            .remove_events(subscription_id)
            .await
            .map_err(|e| {
                log::warn!(
                    "Failed to remove events related to subscription [{}].",
                    subscription_id
                )
            });
        // TODO: We could remove all resources related to Proposals.
        Ok(())
    }

    pub async fn counter_proposal(
        &self,
        subscription_id: &SubscriptionId,
        prev_proposal_id: &ProposalId,
        proposal: &ClientProposal,
    ) -> Result<ProposalId, ProposalError> {
        let (new_proposal, is_initial) = self
            .common
            .counter_proposal(subscription_id, prev_proposal_id, proposal)
            .await?;

        let proposal_id = new_proposal.body.id.clone();
        // Send Proposal to Provider. Note that it can be either our first communication with
        // Provider or we negotiated with him already, so we need to send different message in each
        // of these cases.
        match is_initial {
            true => self.api.initial_proposal(new_proposal).await,
            false => self.api.counter_proposal(new_proposal).await,
        }
        .map_err(|e| ProposalError::FailedSendProposal(prev_proposal_id.clone(), e))?;

        Ok(proposal_id)
    }

    pub async fn query_events(
        &self,
        subscription_id: &SubscriptionId,
        timeout: f32,
        max_events: Option<i32>,
    ) -> Result<Vec<RequestorEvent>, QueryEventsError> {
        let events = self
            .common
            .query_events(subscription_id, timeout, max_events, OwnerType::Requestor)
            .await?;

        // Map model events to client RequestorEvent.
        let db = self.db();
        Ok(futures::stream::iter(events)
            .then(|event| event.into_client_requestor_event(&db))
            .inspect(|result| {
                if let Err(error) = result {
                    log::warn!("Error converting event to client type: {}", error);
                }
            })
            .filter_map(|event| async move { event.ok() })
            .collect::<Vec<RequestorEvent>>()
            .await)
    }

    pub async fn create_agreement(
        &self,
        id: Identity,
        proposal_id: &ProposalId,
        valid_to: DateTime<Utc>,
    ) -> Result<AgreementId, AgreementError> {
        // TODO: Check if we are owner of Proposal
        let offer_proposal_id = proposal_id;
        let offer_proposal = self
            .common
            .get_proposal(offer_proposal_id)
            .await
            .map_err(|e| AgreementError::from(proposal_id, e))?;

        let demand_proposal_id = offer_proposal
            .body
            .prev_proposal_id
            .clone()
            .ok_or_else(|| AgreementError::NoNegotiations(offer_proposal_id.clone()))?;
        let demand_proposal = self
            .common
            .get_proposal(&demand_proposal_id)
            .await
            .map_err(|e| AgreementError::from(proposal_id, e))?;

        let agreement = Agreement::new(
            demand_proposal,
            offer_proposal,
            valid_to.naive_utc(),
            OwnerType::Requestor,
        );
        let id = agreement.id.clone();
        self.db()
            .as_dao::<AgreementDao>()
            .save(agreement)
            .await
            .map_err(|e| AgreementError::FailedSaveAgreement(proposal_id.clone(), e))?;
        Ok(id)
    }

    fn db(&self) -> DbExecutor {
        self.common.db.clone()
    }
}

pub async fn proposal_receiver_thread(
    db: DbExecutor,
    mut proposal_receiver: UnboundedReceiver<DraftProposal>,
    notifier: EventNotifier,
) {
    while let Some(proposal) = proposal_receiver.recv().await {
        let db = db.clone();
        let notifier = notifier.clone();
        match async move {
            log::info!("Received proposal from matcher. Adding to events queue.");

            // Add proposal to database together with Negotiation record.
            let proposal = Proposal::new_initial(proposal.demand, proposal.offer);
            let proposal = db
                .as_dao::<ProposalDao>()
                .save_initial_proposal(proposal)
                .await?;

            // Create Proposal Event and add it to queue (database).
            let subscription_id = proposal.negotiation.subscription_id.clone();
            db.as_dao::<EventsDao>()
                .add_proposal_event(proposal, OwnerType::Requestor)
                .await?;

            // Send channel message to wake all query_events waiting for proposals.
            notifier.notify(&subscription_id).await;
            DbResult::<()>::Ok(())
        }
        .await
        {
            Err(error) => log::warn!("Failed to add proposal. Error: {}", error),
            Ok(_) => (),
        }
    }
}
