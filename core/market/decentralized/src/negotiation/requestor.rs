use futures::stream::{self, StreamExt};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::db::dao::{EventsDao, ProposalDao};
use crate::db::models::EventError;
use crate::db::models::Proposal as ModelProposal;
use crate::db::models::{Demand as ModelDemand, SubscriptionId};
use crate::db::models::{Offer as ModelOffer, ProposalExt};
use crate::matcher::DraftProposal;

use ya_client::model::market::event::RequestorEvent;
use ya_persistence::executor::DbExecutor;

use super::errors::{NegotiationError, NegotiationInitError, ProposalError, QueryEventsError};
use crate::db::DbResult;

/// Requestor part of negotiation logic.
/// TODO: Too long name.
pub struct RequestorNegotiationEngine {
    db: DbExecutor,
}

impl RequestorNegotiationEngine {
    pub fn new(
        db: DbExecutor,
        proposal_receiver: UnboundedReceiver<DraftProposal>,
    ) -> Result<Arc<RequestorNegotiationEngine>, NegotiationInitError> {
        let engine = RequestorNegotiationEngine { db: db.clone() };
        tokio::spawn(proposal_receiver_thread(db, proposal_receiver));
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
        max_events: i32,
    ) -> Result<Vec<RequestorEvent>, QueryEventsError> {
        let subscription_id = SubscriptionId::from_str(subscription_id)?;
        let events = self
            .db
            .as_dao::<EventsDao>()
            .take_requestor_events(&subscription_id, max_events)
            .await?;

        // Map model events to client RequestorEvent.
        let results = futures::stream::iter(events)
            .then(|event| event.into_client_requestor_event(&self.db))
            .collect::<Vec<Result<RequestorEvent, EventError>>>()
            .await;

        // Filter errors. Can we do something better with errors, than logging them?
        let events = results
            .into_iter()
            .inspect(|result| {
                if let Err(error) = result {
                    log::warn!("Error converting event to client type: {}", error);
                }
            })
            .filter_map(|event| event.ok())
            .collect::<Vec<RequestorEvent>>();

        // TODO: If there were no events:
        //  - Spawn future waiting for timeout
        //  - Wait either for timeout or for incoming message about new event.
        Ok(events)
    }
}

pub async fn proposal_receiver_thread(
    db: DbExecutor,
    mut proposal_receiver: UnboundedReceiver<DraftProposal>,
) {
    while let Some(proposal) = proposal_receiver.recv().await {
        let db = db.clone();
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
            db.as_dao::<EventsDao>()
                .add_requestor_event(proposal, negotiation)
                .await?;
            // TODO: Send channel message to wake all query_events waiting for proposals.
            //  Channel should send subscription id related to proposal.
            DbResult::<()>::Ok(())
        }
        .await
        {
            Err(error) => log::warn!("Failed to add proposal. Error: {}", error),
            Ok(_) => (),
        }
    }
}
