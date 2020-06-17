use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::db::dao::ProposalDao;
use crate::db::models::Demand as ModelDemand;
use crate::db::models::Offer as ModelOffer;
use crate::db::models::Proposal as ModelProposal;
use crate::matcher::DraftProposal;

use ya_client::model::market::event::ProviderEvent;
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
    ) -> Result<Vec<ProviderEvent>, QueryEventsError> {
        // TODO: Fetch events from database. Remove them in the same transaction.
        // TODO: If there were no events:
        //  - Spawn future waiting for timeout
        //  - Wait either for timeout or for incoming message about new event.
        Ok(vec![])
    }
}

pub async fn proposal_receiver_thread(
    db: DbExecutor,
    mut proposal_receiver: UnboundedReceiver<DraftProposal>,
) {
    while let Some(proposal) = proposal_receiver.recv().await {
        let db = db.clone();
        match async move {
            log::info!("Received proposal from matcher.");

            // TODO: Add proposal to database together with Negotiation record.
            let proposal = db
                .as_dao::<ProposalDao>()
                .new_initial_proposal(proposal.demand, proposal.offer)
                .await?;

            // TODO: Create Proposal Event and add it to queue (database)
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
