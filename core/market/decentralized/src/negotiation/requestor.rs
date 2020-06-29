use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::db::models::{Demand as ModelDemand, SubscriptionId};
use ya_client::model::market::Proposal;
use ya_persistence::executor::DbExecutor;

use super::errors::{NegotiationError, NegotiationInitError};
use crate::protocol::negotiation::messages::{
    AgreementApproved, AgreementRejected, ProposalReceived, ProposalRejected,
};
use crate::protocol::negotiation::requestor::NegotiationApi;

/// Requestor part of negotiation logic.
/// TODO: Too long name.
pub struct RequestorNegotiationEngine {
    api: NegotiationApi,
    db: DbExecutor,
    proposal_receiver: UnboundedReceiver<Proposal>,
}

impl RequestorNegotiationEngine {
    pub fn new(
        db: DbExecutor,
        proposal_receiver: UnboundedReceiver<Proposal>,
    ) -> Result<Arc<RequestorNegotiationEngine>, NegotiationInitError> {
        let api = NegotiationApi::new(
            move |_caller: String, msg: ProposalReceived| async move { unimplemented!() },
            move |_caller: String, msg: ProposalRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementApproved| async move { unimplemented!() },
            move |caller: String, msg: AgreementRejected| async move { unimplemented!() },
        );

        let engine = RequestorNegotiationEngine {
            api,
            db,
            proposal_receiver,
        };
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
        // TODO: Implement
        Ok(())
    }
}
