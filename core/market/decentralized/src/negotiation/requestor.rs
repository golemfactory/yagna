use chrono::Utc;
use futures::stream::{self, StreamExt};
use num_traits::real::Real;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;

use super::errors::{NegotiationError, NegotiationInitError, ProposalError, QueryEventsError};
use super::EventNotifier;
use crate::db::dao::{EventsDao, ProposalDao};
use crate::db::models::EventError;
use crate::db::models::Proposal as ModelProposal;
use crate::db::models::{Demand as ModelDemand, SubscriptionId};
use crate::db::models::{Offer as ModelOffer, ProposalExt};
use crate::db::DbResult;
use crate::matcher::DraftProposal;

use crate::negotiation::notifier::NotifierError;
use ya_client::model::market::event::RequestorEvent;
use ya_client::model::ErrorMessage;
use ya_persistence::executor::DbExecutor;

/// Requestor part of negotiation logic.
/// TODO: Too long name.
pub struct RequestorNegotiationEngine {
    db: DbExecutor,
    notifier: EventNotifier,
}

impl RequestorNegotiationEngine {
    pub fn new(
        db: DbExecutor,
        proposal_receiver: UnboundedReceiver<DraftProposal>,
    ) -> Result<Arc<RequestorNegotiationEngine>, NegotiationInitError> {
        let notifier = EventNotifier::new();
        let engine = RequestorNegotiationEngine {
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
        Ok(())
    }

    pub async fn subscribe_demand(&self, demand: &ModelDemand) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }

    pub async fn unsubscribe_demand(
        &self,
        subscription_id: &String,
    ) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }

    pub async fn query_events(
        &self,
        subscription_id: &String,
        timeout: f32,
        max_events: Option<i32>,
    ) -> Result<Vec<RequestorEvent>, QueryEventsError> {
        let subscription_id = SubscriptionId::from_str(subscription_id)?;
        let mut timeout = Duration::from_millis((1000.0 * timeout) as u64);
        let stop_time = Instant::now() + timeout;
        let max_events = max_events.unwrap_or(i32::max_value());

        if max_events < 0 {
            Err(QueryEventsError::InvalidMaxEvents(max_events))?
        } else if max_events == 0 {
            return Ok(vec![]);
        }

        loop {
            let events = get_events_from_db(&self.db, &subscription_id, max_events).await?;
            if events.len() > 0 {
                return Ok(events);
            }

            timeout = stop_time - Instant::now();

            if let Err(error) = self
                .notifier
                .wait_for_event_with_timeout(&subscription_id, timeout)
                .await
            {
                return match error {
                    NotifierError::Timeout(_) => Ok(vec![]),
                    NotifierError::ChannelClosed(_) => {
                        Err(QueryEventsError::InternalError(format!("{}", error)))
                    }
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
            let ProposalExt {
                proposal,
                negotiation,
            } = db
                .as_dao::<ProposalDao>()
                .new_initial_proposal(proposal.demand, proposal.offer)
                .await?;

            // Create Proposal Event and add it to queue (database).
            let subscription_id = negotiation.subscription_id.clone();
            db.as_dao::<EventsDao>()
                .add_requestor_event(proposal, negotiation)
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
