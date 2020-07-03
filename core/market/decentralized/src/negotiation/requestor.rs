use futures::stream::StreamExt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;

use super::errors::{NegotiationError, NegotiationInitError, QueryEventsError};
use super::EventNotifier;
use crate::db::dao::{EventsDao, ProposalDao};
use crate::db::models::{Demand as ModelDemand, SubscriptionId};
use crate::db::models::{EventError, OwnerType};
use crate::db::DbResult;
use crate::matcher::DraftProposal;

use ya_client::model::market::event::RequestorEvent;
use ya_client::model::market::proposal::Proposal as ClientProposal;
use ya_persistence::executor::DbExecutor;

use crate::negotiation::notifier::NotifierError;
use crate::negotiation::ProposalError;
use crate::protocol::negotiation::messages::{
    AgreementApproved, AgreementRejected, ProposalReceived, ProposalRejected,
};
use crate::protocol::negotiation::requestor::NegotiationApi;

/// Requestor part of negotiation logic.
pub struct RequestorBroker {
    api: NegotiationApi,
    db: DbExecutor,
    notifier: EventNotifier,
}

impl RequestorBroker {
    pub fn new(
        db: DbExecutor,
        proposal_receiver: UnboundedReceiver<DraftProposal>,
    ) -> Result<Arc<RequestorBroker>, NegotiationInitError> {
        let api = NegotiationApi::new(
            move |_caller: String, msg: ProposalReceived| async move { unimplemented!() },
            move |_caller: String, msg: ProposalRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementApproved| async move { unimplemented!() },
            move |caller: String, msg: AgreementRejected| async move { unimplemented!() },
        );

        let notifier = EventNotifier::new();
        let engine = RequestorBroker {
            api,
            db: db.clone(),
            notifier: notifier.clone(),
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
        self.notifier.stop_notifying(subscription_id).await;

        // We can ignore error, if removing events failed, because they will be never
        // queried again and don't collide with other subscriptions.
        let _ = self
            .db
            .as_dao::<EventsDao>()
            .remove_requestor_events(subscription_id)
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
        prev_proposal_id: &str,
        proposal: &ClientProposal,
    ) -> Result<String, ProposalError> {
        // TODO: Everything should happen under transaction.
        // TODO: Check if subscription is active
        // TODO: Check if this proposal wasn't already countered.
        let prev_proposal = self
            .db
            .as_dao::<ProposalDao>()
            .get_proposal(prev_proposal_id)
            .await
            .map_err(|e| {
                ProposalError::FailedGetProposal(prev_proposal_id.to_string(), e.to_string())
            })?
            .ok_or_else(|| {
                ProposalError::ProposalNotFound(
                    prev_proposal_id.to_string(),
                    subscription_id.clone(),
                )
            })?;

        if &prev_proposal.negotiation.subscription_id != subscription_id {
            Err(ProposalError::ProposalNotFound(
                prev_proposal_id.to_string(),
                subscription_id.clone(),
            ))?
        }

        let is_initial = prev_proposal.body.prev_proposal_id.is_none();
        let new_proposal = prev_proposal.counter_with(proposal);
        let proposal_id = new_proposal.body.id.clone();
        self.db
            .as_dao::<ProposalDao>()
            .save_proposal(&new_proposal)
            .await
            .map_err(|e| ProposalError::FailedSaveProposal(prev_proposal_id.to_string(), e))?;

        // Send Proposal to Provider. Note that it can be either our first communication with
        // Provider or we negotiated with him already, so we need to send different message in each
        // of these cases.
        match is_initial {
            true => self.api.initial_proposal(new_proposal).await,
            false => self.api.counter_proposal(new_proposal).await,
        }
        .map_err(|e| ProposalError::FailedSendProposal(prev_proposal_id.to_string(), e))?;

        Ok(proposal_id)
    }

    pub async fn query_events(
        &self,
        subscription_id: &SubscriptionId,
        timeout: f32,
        max_events: Option<i32>,
    ) -> Result<Vec<RequestorEvent>, QueryEventsError> {
        let mut timeout = Duration::from_secs_f32(timeout.max(0.0));
        let stop_time = Instant::now() + timeout;
        let max_events = max_events.unwrap_or(i32::max_value());

        if max_events < 0 {
            Err(QueryEventsError::InvalidMaxEvents(max_events))?
        } else if max_events == 0 {
            return Ok(vec![]);
        }

        loop {
            let events = get_events_from_db(&self.db, subscription_id, max_events).await?;
            if events.len() > 0 {
                return Ok(events);
            }

            // Solves panic 'supplied instant is later than self'.
            if stop_time < Instant::now() {
                return Ok(vec![]);
            }
            timeout = stop_time - Instant::now();

            if let Err(error) = self
                .notifier
                .wait_for_event_with_timeout(subscription_id, timeout)
                .await
            {
                return match error {
                    NotifierError::Timeout(_) => Ok(vec![]),
                    NotifierError::ChannelClosed(_) => {
                        Err(QueryEventsError::InternalError(format!("{}", error)))
                    }
                    NotifierError::Unsubscribed(id) => Err(QueryEventsError::Unsubscribed(id)),
                };
            }
            // Ok result means, that event with required subscription id was added.
            // We can go to next loop to get this event from db. But still we aren't sure
            // that list won't be empty, because other query_events calls can wait for the same event.
        }
    }
}

async fn get_events_from_db(
    db: &DbExecutor,
    subscription_id: &SubscriptionId,
    max_events: i32,
) -> Result<Vec<RequestorEvent>, QueryEventsError> {
    let events = db
        .as_dao::<EventsDao>()
        .take_requestor_events(subscription_id, max_events)
        .await?;

    // Map model events to client RequestorEvent.
    let results = futures::stream::iter(events)
        .then(|event| event.into_client_requestor_event(&db))
        .collect::<Vec<Result<RequestorEvent, EventError>>>()
        .await;

    // Filter errors. Can we do something better with errors, than logging them?
    Ok(results
        .into_iter()
        .inspect(|result| {
            if let Err(error) = result {
                log::warn!("Error converting event to client type: {}", error);
            }
        })
        .filter_map(|event| event.ok())
        .collect::<Vec<RequestorEvent>>())
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
            let proposal = db
                .as_dao::<ProposalDao>()
                .new_initial_proposal(proposal.demand, proposal.offer)
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
