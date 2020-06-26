use std::sync::Arc;

use super::errors::{NegotiationApiInitError, ProposalError, AgreementError};
use super::super::callbacks::HandlerSlot;
use super::messages::{ProposalReceived, ProposalRejected, InitialProposalReceived};


/// Responsible for communication with markets on other nodes
/// during negotiation phase.
#[derive(Clone)]
pub struct NegotiationApi {
    inner: Arc<NegotiationImpl>
}

struct NegotiationImpl {
    initial_proposal_received: HandlerSlot<InitialProposalReceived>,
    proposal_received: HandlerSlot<ProposalReceived>,
    proposal_rejected: HandlerSlot<ProposalRejected>,
}

impl NegotiationApi {
    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), NegotiationApiInitError> {
        unimplemented!()
    }

    pub async fn counter_proposal(
        &self
    ) -> Result<(), ProposalError> {
        unimplemented!()
    }

    pub async fn reject_proposal(
        &self
    ) -> Result<(), ProposalError> {
        unimplemented!()
    }

    pub async fn approve_agreement(
        &self
    ) -> Result<(), AgreementError> {
        unimplemented!()
    }

    pub async fn reject_agreement(
        &self
    ) -> Result<(), AgreementError> {
        unimplemented!()
    }
}

