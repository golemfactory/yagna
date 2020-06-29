use std::sync::Arc;

use super::super::callbacks::{CallbackHandler, HandlerSlot};
use super::errors::{AgreementError, NegotiationApiInitError, ProposalError};
use super::messages::*;
use super::messages::{
    AgreementReceived, AgreementRejected, InitialProposalReceived, ProposalReceived,
    ProposalRejected,
};

use ya_service_bus::typed as bus;

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

    pub async fn on_initial_proposal_received(
        self,
        caller: String,
        msg: InitialProposalReceived,
    ) -> Result<(), ProposalError> {
        log::debug!(
            "Negotiation API: Received initial proposal [{}] from [{}].",
            &msg.proposal_id,
            &caller
        );
        self.inner.initial_proposal_received.call(caller, msg).await
    }

    pub async fn on_proposal_received(
        self,
        caller: String,
        msg: ProposalReceived,
    ) -> Result<(), ProposalError> {
        log::debug!(
            "Negotiation API: Received proposal [{}] from [{}].",
            &msg.proposal_id,
            &caller
        );
        self.inner.proposal_received.call(caller, msg).await
    }

    pub async fn on_proposal_rejected(
        self,
        caller: String,
        msg: ProposalRejected,
    ) -> Result<(), ProposalError> {
        log::debug!(
            "Negotiation API: Proposal [{}] rejected by [{}].",
            &msg.proposal_id,
            &caller
        );
        self.inner.proposal_rejected.call(caller, msg).await
    }

    pub async fn on_agreement_received(
        self,
        caller: String,
        msg: AgreementReceived,
    ) -> Result<(), AgreementError> {
        log::debug!(
            "Negotiation API: Agreement proposal [{}] sent by [{}].",
            &msg.agreement_id,
            &caller
        );
        self.inner.agreement_received.call(caller, msg).await
    }

    pub async fn on_agreement_cancelled(
        self,
        caller: String,
        msg: AgreementRejected,
    ) -> Result<(), AgreementError> {
        log::debug!(
            "Negotiation API: Agreement [{}] cancelled by [{}].",
            &msg.agreement_id,
            &caller
        );
        self.inner.agreement_cancelled.call(caller, msg).await
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), NegotiationApiInitError> {
        let myself = self.clone();
        let _ = bus::bind_with_caller(
            &provider::proposal_addr(public_prefix),
            move |caller: String, msg: InitialProposalReceived| {
                let myself = myself.clone();
                myself.on_initial_proposal_received(caller, msg)
            },
        );

        let myself = self.clone();
        let _ = bus::bind_with_caller(
            &provider::proposal_addr(public_prefix),
            move |caller: String, msg: ProposalReceived| {
                let myself = myself.clone();
                myself.on_proposal_received(caller, msg)
            },
        );

        let myself = self.clone();
        let _ = bus::bind_with_caller(
            &provider::proposal_addr(public_prefix),
            move |caller: String, msg: ProposalRejected| {
                let myself = myself.clone();
                myself.on_proposal_rejected(caller, msg)
            },
        );

        let myself = self.clone();
        let _ = bus::bind_with_caller(
            &provider::agreement_addr(public_prefix),
            move |caller: String, msg: AgreementReceived| {
                let myself = myself.clone();
                myself.on_agreement_received(caller, msg)
            },
        );

        let myself = self.clone();
        let _ = bus::bind_with_caller(
            &provider::agreement_addr(public_prefix),
            move |caller: String, msg: AgreementRejected| {
                let myself = myself.clone();
                myself.on_agreement_cancelled(caller, msg)
            },
        );
        Ok(())
    }
}
