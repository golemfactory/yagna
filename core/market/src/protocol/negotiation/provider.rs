use std::sync::Arc;
use std::time::Duration;

use ya_client::model::market::Reason;
use ya_client::model::NodeId;
use ya_core_model::market::BUS_ID;
use ya_net::{self as net, RemoteEndpoint};
use ya_service_bus::{typed::ServiceBinder, RpcEndpoint};

use crate::db::model::{Agreement, Owner, Proposal};

use super::super::callback::{CallbackHandler, HandlerSlot};
use super::error::{
    AgreementProtocolError, CounterProposalError, GsbAgreementError, GsbProposalError,
    NegotiationApiInitError, TerminateAgreementError,
};
use super::messages::{
    provider, requestor, AgreementApproved, AgreementCancelled, AgreementCommitted,
    AgreementReceived, AgreementRejected, AgreementTerminated, InitialProposalReceived,
    ProposalContent, ProposalReceived, ProposalRejected,
};
use crate::protocol::negotiation::error::{
    CommitAgreementError, ProposeAgreementError, RejectProposalError,
};
use chrono::NaiveDateTime;

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
    agreement_terminated: HandlerSlot<AgreementTerminated>,
    agreement_committed: HandlerSlot<AgreementCommitted>,
}

// TODO: Most of these functions don't need to be members of NegotiationApi.
//  We should make them plain functions in `provider` module, since it doesn't
//  seem, that they will ever need self.
impl NegotiationApi {
    pub fn new(
        initial_proposal_received: impl CallbackHandler<InitialProposalReceived>,
        proposal_received: impl CallbackHandler<ProposalReceived>,
        proposal_rejected: impl CallbackHandler<ProposalRejected>,
        agreement_received: impl CallbackHandler<AgreementReceived>,
        agreement_cancelled: impl CallbackHandler<AgreementCancelled>,
        agreement_terminated: impl CallbackHandler<AgreementTerminated>,
        agreement_committed: impl CallbackHandler<AgreementCommitted>,
    ) -> NegotiationApi {
        let negotiation_impl = NegotiationImpl {
            initial_proposal_received: HandlerSlot::new(initial_proposal_received),
            proposal_received: HandlerSlot::new(proposal_received),
            proposal_rejected: HandlerSlot::new(proposal_rejected),
            agreement_received: HandlerSlot::new(agreement_received),
            agreement_cancelled: HandlerSlot::new(agreement_cancelled),
            agreement_terminated: HandlerSlot::new(agreement_terminated),
            agreement_committed: HandlerSlot::new(agreement_committed),
        };
        NegotiationApi {
            inner: Arc::new(negotiation_impl),
        }
    }

    pub async fn counter_proposal(&self, proposal: Proposal) -> Result<(), CounterProposalError> {
        let proposal_id = proposal.body.id.clone();
        log::debug!(
            "Counter proposal [{}] sent by [{}].",
            proposal_id,
            proposal.negotiation.requestor_id
        );

        let prev_proposal_id = proposal.body.prev_proposal_id.clone();
        if prev_proposal_id.is_none() {
            Err(CounterProposalError::NoPrevious(proposal.body.id.clone()))?
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
            .await
            .map_err(|e| GsbProposalError(e.to_string(), proposal_id))??;
        Ok(())
    }

    pub async fn reject_proposal(
        &self,
        id: NodeId,
        proposal: &Proposal,
        reason: Option<Reason>,
    ) -> Result<(), RejectProposalError> {
        net::from(id)
            .to(proposal.negotiation.requestor_id)
            .service(&requestor::proposal_addr(BUS_ID))
            .send(ProposalRejected::of(proposal, reason))
            .await
            .map_err(|e| GsbProposalError(e.to_string(), proposal.body.id.clone()))??;
        Ok(())
    }

    pub async fn approve_agreement(
        &self,
        agreement: &Agreement,
        timeout: f32,
    ) -> Result<(), AgreementProtocolError> {
        let timeout = Duration::from_secs_f32(timeout.max(0.0));
        let id = agreement.id.clone();

        let msg = AgreementApproved {
            agreement_id: id.clone(),
            signature: agreement
                .approved_signature
                .clone()
                .ok_or(AgreementProtocolError::NotSigned(id.clone()))?,
            approved_ts: agreement
                .approved_ts
                .ok_or(AgreementProtocolError::NoApprovalTimestamp(id.clone()))?,
        };
        let net_send_fut = net::from(agreement.provider_id)
            .to(agreement.requestor_id)
            .service(&requestor::agreement_addr(BUS_ID))
            .send(msg);
        tokio::time::timeout(timeout, net_send_fut)
            .await
            .map_err(|_| AgreementProtocolError::Timeout(id.clone()))?
            .map_err(|e| GsbAgreementError(e.to_string(), id.clone()))??;
        Ok(())
    }

    pub async fn reject_agreement(
        &self,
        agreement: &Agreement,
        reason: Option<Reason>,
        timestamp: NaiveDateTime,
    ) -> Result<(), AgreementProtocolError> {
        let msg = AgreementRejected {
            agreement_id: agreement.id.clone(),
            reason,
            rejection_ts: timestamp,
        };
        net::from(agreement.provider_id)
            .to(agreement.requestor_id)
            .service(&requestor::agreement_addr(BUS_ID))
            .send(msg)
            .await
            .map_err(|e| GsbAgreementError(e.to_string(), agreement.id.clone()))??;
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
            .call(caller, msg.translate(Owner::Provider))
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
        log::trace!(
            "Negotiation API: Received proposal [{}] from [{}].",
            &msg.proposal.proposal_id,
            &caller
        );
        self.inner
            .proposal_received
            .call(caller, msg.translate(Owner::Provider))
            .await
    }

    async fn on_proposal_rejected(
        self,
        caller: String,
        msg: ProposalRejected,
    ) -> Result<(), RejectProposalError> {
        log::debug!(
            "Negotiation API: Proposal [{}] rejected by [{}].",
            &msg.proposal_id,
            &caller
        );
        self.inner
            .proposal_rejected
            .call(caller, msg.translate(Owner::Provider))
            .await
    }

    async fn on_agreement_received(
        self,
        caller: String,
        msg: AgreementReceived,
    ) -> Result<(), ProposeAgreementError> {
        log::debug!(
            "Negotiation API: Agreement proposal [{}] sent by [{}].",
            &msg.agreement_id,
            &caller
        );
        self.inner
            .agreement_received
            .call(caller, msg.translate(Owner::Provider))
            .await
    }

    async fn on_agreement_cancelled(
        self,
        caller: String,
        msg: AgreementCancelled,
    ) -> Result<(), AgreementProtocolError> {
        log::debug!(
            "Negotiation API: Agreement [{}] cancelled by [{}].",
            &msg.agreement_id,
            &caller
        );
        self.inner
            .agreement_cancelled
            .call(caller, msg.translate(Owner::Provider))
            .await
    }

    async fn on_agreement_terminated(
        self,
        caller: String,
        msg: AgreementTerminated,
    ) -> Result<(), TerminateAgreementError> {
        log::debug!(
            "Negotiation API: Agreement [{}] terminated by [{}].",
            &msg.agreement_id,
            &caller
        );
        self.inner
            .agreement_terminated
            .call(caller, msg.translate(Owner::Provider))
            .await
    }

    async fn on_agreement_committed(
        self,
        caller: String,
        msg: AgreementCommitted,
    ) -> Result<(), CommitAgreementError> {
        log::debug!(
            "Negotiation API: Agreement [{}] committed by [{}].",
            &msg.agreement_id,
            &caller
        );
        self.inner
            .agreement_committed
            .call(caller, msg.translate(Owner::Provider))
            .await
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        _local_prefix: &str,
    ) -> Result<(), NegotiationApiInitError> {
        log::info!("Negotiation (Provider) protocol version: mk1");

        ServiceBinder::new(&provider::proposal_addr(public_prefix), &(), self.clone())
            .bind_with_processor(
                move |_, myself, caller: String, msg: InitialProposalReceived| {
                    let myself = myself;
                    myself.on_initial_proposal_received(caller, msg)
                },
            )
            .bind_with_processor(move |_, myself, caller: String, msg: ProposalReceived| {
                let myself = myself;
                myself.on_proposal_received(caller, msg)
            })
            .bind_with_processor(move |_, myself, caller: String, msg: ProposalRejected| {
                let myself = myself;
                myself.on_proposal_rejected(caller, msg)
            });

        ServiceBinder::new(&provider::agreement_addr(public_prefix), &(), self.clone())
            .bind_with_processor(move |_, myself, caller: String, msg: AgreementReceived| {
                let myself = myself;
                myself.on_agreement_received(caller, msg)
            })
            .bind_with_processor(move |_, myself, caller: String, msg: AgreementCancelled| {
                let myself = myself;
                myself.on_agreement_cancelled(caller, msg)
            })
            .bind_with_processor(move |_, myself, caller: String, msg: AgreementTerminated| {
                let myself = myself;
                myself.on_agreement_terminated(caller, msg)
            })
            .bind_with_processor(move |_, myself, caller: String, msg: AgreementCommitted| {
                let myself = myself;
                myself.on_agreement_committed(caller, msg)
            });
        Ok(())
    }
}
