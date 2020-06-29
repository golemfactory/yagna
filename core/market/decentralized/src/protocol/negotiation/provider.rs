use std::sync::Arc;

use super::super::callbacks::{CallbackHandler, HandlerSlot};
use super::errors::{AgreementError, NegotiationApiInitError, ProposalError};
use super::messages::{
    AgreementReceived, AgreementRejected, InitialProposalReceived, ProposalReceived,
    ProposalRejected,
};

/// Responsible for communication with markets on other nodes
/// during negotiation phase.
#[derive(Clone)]
pub struct NegotiationApi {
    inner: Arc<NegotiationImpl>,
}

struct NegotiationImpl {
    initial_proposal_received: HandlerSlot<InitialProposalReceived>,
    proposal_received: HandlerSlot<ProposalReceived>,
    proposal_rejected: HandlerSlot<ProposalRejected>,
    agreement_received: HandlerSlot<AgreementReceived>,
    agreement_cancelled: HandlerSlot<AgreementRejected>,
}

impl NegotiationApi {
    pub fn new(
        initial_proposal_received: impl CallbackHandler<InitialProposalReceived>,
        proposal_received: impl CallbackHandler<ProposalReceived>,
        proposal_rejected: impl CallbackHandler<ProposalRejected>,
        agreement_received: impl CallbackHandler<AgreementReceived>,
        agreement_cancelled: impl CallbackHandler<AgreementRejected>,
    ) -> NegotiationApi {
        let negotiation_impl = NegotiationImpl {
            initial_proposal_received: HandlerSlot::new(initial_proposal_received),
            proposal_received: HandlerSlot::new(proposal_received),
            proposal_rejected: HandlerSlot::new(proposal_rejected),
            agreement_received: HandlerSlot::new(agreement_received),
            agreement_cancelled: HandlerSlot::new(agreement_cancelled),
        };
        NegotiationApi {
            inner: Arc::new(negotiation_impl),
        }
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), NegotiationApiInitError> {
        // TODO: Implement.
        Ok(())
    }

    pub async fn counter_proposal(&self) -> Result<(), ProposalError> {
        unimplemented!()
    }

    pub async fn reject_proposal(&self) -> Result<(), ProposalError> {
        unimplemented!()
    }

    pub async fn approve_agreement(&self) -> Result<(), AgreementError> {
        unimplemented!()
    }

    pub async fn reject_agreement(&self) -> Result<(), AgreementError> {
        unimplemented!()
    }
}
