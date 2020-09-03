use futures::future::TryFutureExt;
use std::sync::Arc;

use ya_client::model::NodeId;
use ya_core_model::market::BUS_ID;
use ya_net::{self as net, RemoteEndpoint};
use ya_service_bus::{typed::ServiceBinder, RpcEndpoint};

use crate::db::model::{Agreement, AgreementId, OwnerType, Proposal, ProposalId};

use super::super::callback::{CallbackHandler, HandlerSlot};
use super::error::{
    AgreementError, ApproveAgreementError, CounterProposalError, NegotiationApiInitError,
    ProposalError,
};
use super::messages::{
    provider, requestor, AgreementApproved, AgreementCancelled, AgreementReceived,
    AgreementRejected, InitialProposalReceived, ProposalContent, ProposalReceived,
    ProposalRejected,
};

/// Responsible for communication with markets on other nodes
/// during negotiation phase.
#[derive(Clone)]
pub struct NegotiationApi {
    inner: Arc<NegotiationImpl>,
}

struct NegotiationImpl {
    proposal_received: HandlerSlot<ProposalReceived>,
    proposal_rejected: HandlerSlot<ProposalRejected>,
    agreement_approved: HandlerSlot<AgreementApproved>,
    agreement_rejected: HandlerSlot<AgreementRejected>,
}

impl NegotiationApi {
    pub fn new(
        proposal_received: impl CallbackHandler<ProposalReceived>,
        proposal_rejected: impl CallbackHandler<ProposalRejected>,
        agreement_approved: impl CallbackHandler<AgreementApproved>,
        agreement_rejected: impl CallbackHandler<AgreementRejected>,
    ) -> NegotiationApi {
        let negotiation_impl = NegotiationImpl {
            proposal_received: HandlerSlot::new(proposal_received),
            proposal_rejected: HandlerSlot::new(proposal_rejected),
            agreement_approved: HandlerSlot::new(agreement_approved),
            agreement_rejected: HandlerSlot::new(agreement_rejected),
        };
        NegotiationApi {
            inner: Arc::new(negotiation_impl),
        }
    }

    /// Sent to provider, when Requestor counters initial proposal
    /// generated by market.
    pub async fn initial_proposal(&self, proposal: Proposal) -> Result<(), CounterProposalError> {
        let proposal_id = proposal.body.id.clone();
        log::debug!(
            "Sending initial proposal [{}] to [{}].",
            proposal_id,
            proposal.negotiation.provider_id
        );

        let content = ProposalContent::from(proposal.body);
        let msg = InitialProposalReceived {
            proposal: content,
            offer_id: proposal.negotiation.offer_id,
            demand_id: proposal.negotiation.demand_id,
        };
        net::from(proposal.negotiation.requestor_id)
            .to(proposal.negotiation.provider_id)
            .service(&provider::proposal_addr(BUS_ID))
            .send(msg)
            .await
            .map_err(|e| CounterProposalError::GsbError(e.to_string(), proposal_id))??;
        Ok(())
    }

    /// Counter proposals used in all other cases, when proposal
    /// is not in initial state.
    pub async fn counter_proposal(&self, proposal: Proposal) -> Result<(), CounterProposalError> {
        let proposal_id = proposal.body.id.clone();
        log::debug!(
            "Counter proposal [{}] sent by [{}].",
            proposal_id,
            proposal.negotiation.provider_id
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
        net::from(proposal.negotiation.requestor_id)
            .to(proposal.negotiation.provider_id)
            .service(&provider::proposal_addr(BUS_ID))
            .send(msg)
            .await
            .map_err(|e| CounterProposalError::GsbError(e.to_string(), proposal_id))??;
        Ok(())
    }

    // TODO: Use model Proposal struct.
    pub async fn reject_proposal(
        &self,
        id: NodeId,
        proposal_id: &ProposalId,
        owner: NodeId,
    ) -> Result<(), ProposalError> {
        let msg = ProposalRejected {
            proposal_id: proposal_id.clone(),
        };
        net::from(id)
            .to(owner)
            .service(&provider::proposal_addr(BUS_ID))
            .send(msg)
            .await
            .map_err(|e| ProposalError::GsbError(e.to_string(), proposal_id.clone()))??;
        Ok(())
    }

    /// Sent to provider, when Requestor will call confirm Agreement.
    pub async fn propose_agreement(&self, agreement: Agreement) -> Result<(), AgreementError> {
        let requestor_id = agreement.requestor_id.clone();
        let provider_id = agreement.provider_id.clone();
        let agreement_id = agreement.id.clone();
        let msg = AgreementReceived { agreement };
        net::from(requestor_id)
            .to(provider_id)
            .service(&provider::agreement_addr(BUS_ID))
            .send(msg)
            .map_err(|e| AgreementError::GsbError(e.to_string(), agreement_id))
            .await??;
        Ok(())
    }

    /// Sent to provider, when Requestor will call cancel Agreement,
    /// while waiting for approval.
    /// TODO: Use model Agreement struct in api.
    pub async fn cancel_agreement(
        &self,
        id: NodeId,
        agreement_id: AgreementId,
        owner: NodeId,
    ) -> Result<(), AgreementError> {
        let msg = AgreementCancelled {
            agreement_id: agreement_id.clone(),
        };
        net::from(id)
            .to(owner)
            .service(&provider::agreement_addr(BUS_ID))
            .send(msg)
            .await
            .map_err(|e| AgreementError::GsbError(e.to_string(), agreement_id))??;
        Ok(())
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
            .call(caller, msg.translate(OwnerType::Requestor))
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
            .call(caller, msg.translate(OwnerType::Requestor))
            .await
    }

    async fn on_agreement_approved(
        self,
        caller: String,
        msg: AgreementApproved,
    ) -> Result<(), ApproveAgreementError> {
        log::debug!(
            "Negotiation API: Agreement [{}] approved by [{}].",
            &msg.agreement_id,
            &caller
        );
        self.inner
            .agreement_approved
            .call(caller, msg.translate(OwnerType::Requestor))
            .await
    }

    async fn on_agreement_rejected(
        self,
        caller: String,
        msg: AgreementRejected,
    ) -> Result<(), AgreementError> {
        log::debug!(
            "Negotiation API: Agreement [{}] rejected by [{}].",
            &msg.agreement_id,
            &caller
        );
        self.inner.agreement_rejected.call(caller, msg).await
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        _local_prefix: &str,
    ) -> Result<(), NegotiationApiInitError> {
        ServiceBinder::new(&requestor::proposal_addr(public_prefix), &(), self.clone())
            .bind_with_processor(move |_, myself, caller: String, msg: ProposalReceived| {
                let myself = myself.clone();
                myself.on_proposal_received(caller, msg)
            })
            .bind_with_processor(move |_, myself, caller: String, msg: ProposalRejected| {
                let myself = myself.clone();
                myself.on_proposal_rejected(caller, msg)
            });

        ServiceBinder::new(&requestor::agreement_addr(public_prefix), &(), self.clone())
            .bind_with_processor(move |_, myself, caller: String, msg: AgreementApproved| {
                let myself = myself.clone();
                myself.on_agreement_approved(caller, msg)
            })
            .bind_with_processor(move |_, myself, caller: String, msg: AgreementRejected| {
                let myself = myself.clone();
                myself.on_agreement_rejected(caller, msg)
            });
        Ok(())
    }
}
