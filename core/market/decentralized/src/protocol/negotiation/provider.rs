use std::sync::Arc;

use super::super::callbacks::{CallbackHandler, HandlerSlot};
use super::errors::{AgreementError, NegotiationApiInitError, ProposalError};
use super::messages::*;
use super::messages::{
    AgreementCancelled, AgreementReceived, AgreementRejected, InitialProposalReceived,
    ProposalReceived, ProposalRejected,
};

use crate::db::models::{OwnerType, Proposal, ProposalId};
use crate::protocol::negotiation::errors::CounterProposalError;

use std::str::FromStr;
use ya_client::model::NodeId;
use ya_core_model::market::BUS_ID;
use ya_net::{self as net, RemoteEndpoint};
use ya_service_bus::typed::ServiceBinder;
use ya_service_bus::RpcEndpoint;

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
    agreement_cancelled: HandlerSlot<AgreementCancelled>,
}

impl NegotiationApi {
    pub fn new(
        initial_proposal_received: impl CallbackHandler<InitialProposalReceived>,
        proposal_received: impl CallbackHandler<ProposalReceived>,
        proposal_rejected: impl CallbackHandler<ProposalRejected>,
        agreement_received: impl CallbackHandler<AgreementReceived>,
        agreement_cancelled: impl CallbackHandler<AgreementCancelled>,
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

    pub async fn counter_proposal(&self, proposal: Proposal) -> Result<(), CounterProposalError> {
        log::debug!(
            "Counter proposal [{}] sent by [{}].",
            proposal.body.id.clone(),
            proposal.negotiation.requestor_id
        );

        let prev_proposal_id = proposal.body.prev_proposal_id.clone();
        if prev_proposal_id.is_none() {
            Err(CounterProposalError::NoPreviousProposal(
                proposal.body.id.clone(),
            ))?
        }

        let content = ProposalContent::from(proposal.body);
        let msg = ProposalReceived {
            proposal: content,
            prev_proposal_id: prev_proposal_id.unwrap(),
        };
        net::from(proposal.negotiation.provider_id)
            .to(proposal.negotiation.requestor_id)
            .service(&requestor::proposal_addr(BUS_ID))
            .send(msg)
            .await??;
        Ok(())
    }

    // TODO: Use model Proposal struct.
    pub async fn reject_proposal(
        &self,
        id: NodeId,
        proposal_id: &str,
        owner: NodeId,
    ) -> Result<(), ProposalError> {
        let msg = ProposalRejected {
            proposal_id: ProposalId::from_str(&proposal_id).unwrap(),
        };
        net::from(id)
            .to(owner)
            .service(&requestor::proposal_addr(BUS_ID))
            .send(msg)
            .await??;
        Ok(())
    }

    /// TODO: pass agreement signature.
    pub async fn approve_agreement(
        &self,
        id: NodeId,
        agreement_id: &str,
        owner: NodeId,
    ) -> Result<(), AgreementError> {
        let msg = AgreementApproved {
            agreement_id: agreement_id.to_string(),
        };
        net::from(id)
            .to(owner)
            .service(&requestor::agreement_addr(BUS_ID))
            .send(msg)
            .await??;
        Ok(())
    }

    pub async fn reject_agreement(
        &self,
        id: NodeId,
        agreement_id: &str,
        owner: NodeId,
    ) -> Result<(), AgreementError> {
        let msg = AgreementRejected {
            agreement_id: agreement_id.to_string(),
        };
        net::from(id)
            .to(owner)
            .service(&requestor::agreement_addr(BUS_ID))
            .send(msg)
            .await??;
        Ok(())
    }

    async fn on_initial_proposal_received(
        self,
        caller: String,
        msg: InitialProposalReceived,
    ) -> Result<(), CounterProposalError> {
        let proposal_id = &msg.proposal.proposal_id.clone();
        log::debug!(
            "Negotiation API: Received initial proposal [{}] from [{}].",
            &proposal_id,
            &caller
        );
        self.inner
            .initial_proposal_received
            .call(caller, msg.translate(OwnerType::Provider))
            .await
            .map_err(|e| {
                log::warn!(
                    "Negotiation API: initial proposal [{}] rejected. Error: {}",
                    proposal_id,
                    &e
                );
                e
            })
    }

    async fn on_proposal_received(
        self,
        caller: String,
        msg: ProposalReceived,
    ) -> Result<(), CounterProposalError> {
        log::debug!(
            "Negotiation API: Received proposal [{}] from [{}].",
            &msg.proposal.proposal_id,
            &caller
        );
        self.inner
            .proposal_received
            .call(caller, msg.translate(OwnerType::Provider))
            .await
    }

    async fn on_proposal_rejected(
        self,
        caller: String,
        msg: ProposalRejected,
    ) -> Result<(), ProposalError> {
        log::debug!(
            "Negotiation API: Proposal [{}] rejected by [{}].",
            &msg.proposal_id,
            &caller
        );
        self.inner
            .proposal_rejected
            .call(caller, msg.translate(OwnerType::Provider))
            .await
    }

    async fn on_agreement_received(
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

    async fn on_agreement_cancelled(
        self,
        caller: String,
        msg: AgreementCancelled,
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
        _private_prefix: &str,
    ) -> Result<(), NegotiationApiInitError> {
        ServiceBinder::new(&provider::proposal_addr(public_prefix), &(), self.clone())
            .bind_with_processor(
                move |_, myself, caller: String, msg: InitialProposalReceived| {
                    let myself = myself.clone();
                    myself.on_initial_proposal_received(caller, msg)
                },
            )
            .bind_with_processor(move |_, myself, caller: String, msg: ProposalReceived| {
                let myself = myself.clone();
                myself.on_proposal_received(caller, msg)
            })
            .bind_with_processor(move |_, myself, caller: String, msg: ProposalRejected| {
                let myself = myself.clone();
                myself.on_proposal_rejected(caller, msg)
            });

        ServiceBinder::new(&provider::agreement_addr(public_prefix), &(), self.clone())
            .bind_with_processor(move |_, myself, caller: String, msg: AgreementReceived| {
                let myself = myself.clone();
                myself.on_agreement_received(caller, msg)
            })
            .bind_with_processor(move |_, myself, caller: String, msg: AgreementCancelled| {
                let myself = myself.clone();
                myself.on_agreement_cancelled(caller, msg)
            });
        Ok(())
    }
}
