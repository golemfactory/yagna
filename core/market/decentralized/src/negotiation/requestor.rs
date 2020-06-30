use tokio::sync::mpsc::UnboundedReceiver;

use crate::db::models::{Demand as ModelDemand, SubscriptionId};
use ya_client::model::market::Proposal;
use ya_persistence::executor::DbExecutor;

use super::errors::{NegotiationError, NegotiationInitError};

/// Requestor part of negotiation logic.
/// TODO: Too long name.
pub struct RequestorNegotiationEngine {
    db: DbExecutor,
    pub proposal_receiver: UnboundedReceiver<Proposal>,
}

impl RequestorNegotiationEngine {
    pub fn new(
        db: DbExecutor,
        proposal_receiver: UnboundedReceiver<Proposal>,
    ) -> Result<RequestorNegotiationEngine, NegotiationInitError> {
        Ok(RequestorNegotiationEngine {
            db,
            proposal_receiver,
        })
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
        subscription_id: &SubscriptionId,
    ) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }
}
