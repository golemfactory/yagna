use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use ya_client::model::market::Reason;
use ya_service_bus::RpcMessage;

use crate::db::model::{AgreementId, DbProposal, Owner, Proposal, ProposalId, SubscriptionId};
use crate::protocol::negotiation::error::{
    CommitAgreementError, ProposeAgreementError, RejectProposalError,
};

use super::super::callback::CallbackMessage;
use super::error::{AgreementProtocolError, CounterProposalError, TerminateAgreementError};

pub mod provider {
    pub fn proposal_addr(prefix: &str) -> String {
        format!(
            "{}/protocol/{}/negotiation/provider/proposal",
            prefix,
            PROTOCOL_VERSION!()
        )
    }

    pub fn agreement_addr(prefix: &str) -> String {
        format!(
            "{}/protocol/{}/negotiation/provider/agreement",
            prefix,
            PROTOCOL_VERSION!()
        )
    }
}

pub mod requestor {
    pub fn proposal_addr(prefix: &str) -> String {
        format!(
            "{}/protocol/{}/negotiation/requestor/proposal",
            prefix,
            PROTOCOL_VERSION!()
        )
    }

    pub fn agreement_addr(prefix: &str) -> String {
        format!(
            "{}/protocol/{}/negotiation/requestor/agreement",
            prefix,
            PROTOCOL_VERSION!()
        )
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProposalContent {
    pub proposal_id: ProposalId,
    pub properties: String,
    pub constraints: String,

    pub creation_ts: NaiveDateTime,
    pub expiration_ts: NaiveDateTime,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalReceived {
    pub prev_proposal_id: ProposalId,
    pub proposal: ProposalContent,
}

impl RpcMessage for ProposalReceived {
    const ID: &'static str = "ProposalReceived";
    type Item = ();
    type Error = CounterProposalError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialProposalReceived {
    pub proposal: ProposalContent,

    pub offer_id: SubscriptionId,
    pub demand_id: SubscriptionId,
}

impl RpcMessage for InitialProposalReceived {
    const ID: &'static str = "InitialProposalReceived";
    type Item = ();
    type Error = CounterProposalError;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalRejected {
    pub proposal_id: ProposalId,
    pub initial: bool,
    pub reason: Option<Reason>,
}

impl RpcMessage for ProposalRejected {
    const ID: &'static str = "ProposalRejected";
    type Item = ();
    type Error = RejectProposalError;
}

impl ProposalRejected {
    pub fn of(proposal: &Proposal, reason: Option<Reason>) -> Self {
        Self {
            proposal_id: proposal.body.id.clone(),
            initial: proposal.body.prev_proposal_id.is_none(),
            reason,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementReceived {
    pub proposal_id: ProposalId,
    pub agreement_id: AgreementId,
    pub creation_ts: NaiveDateTime,
    pub valid_to: NaiveDateTime,
    /// This will be placed in `proposed_signature` Agreement field.
    pub signature: String,
}

impl RpcMessage for AgreementReceived {
    const ID: &'static str = "AgreementReceived";
    type Item = ();
    type Error = ProposeAgreementError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementApproved {
    pub agreement_id: AgreementId,
    /// This will be placed in `approved_signature` Agreement field.
    pub signature: String,
    /// This timestamp will differ from timestamp, when Agreement will be updated in
    /// database to `Approved` state and `AgreementApprovedEvent` timestamp either.
    /// But we can't set it to time value, when state changes to `Approved`, because we
    /// must include this field in signature.
    pub approved_ts: NaiveDateTime,
}

impl RpcMessage for AgreementApproved {
    const ID: &'static str = "AgreementApproved";
    type Item = ();
    type Error = AgreementProtocolError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementRejected {
    pub agreement_id: AgreementId,
    pub reason: Option<Reason>,
    pub rejection_ts: NaiveDateTime,
}

impl RpcMessage for AgreementRejected {
    const ID: &'static str = "AgreementRejected";
    type Item = ();
    type Error = AgreementProtocolError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementCancelled {
    pub agreement_id: AgreementId,
    pub reason: Option<Reason>,
    pub cancellation_ts: NaiveDateTime,
}

impl RpcMessage for AgreementCancelled {
    const ID: &'static str = "AgreementCancelled";
    type Item = ();
    type Error = AgreementProtocolError;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementTerminated {
    pub agreement_id: AgreementId,
    pub reason: Option<Reason>,
    /// Signature for `AgreementTerminatedEvent`.
    pub signature: String,
    /// Termination timestamp, that will be included in signature.
    pub termination_ts: NaiveDateTime,
}

impl RpcMessage for AgreementTerminated {
    const ID: &'static str = "AgreementTerminated";
    type Item = ();
    type Error = TerminateAgreementError;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementCommitted {
    pub agreement_id: AgreementId,
    /// This will be placed in `committed_signature` Agreement field.
    pub signature: String,
}

impl RpcMessage for AgreementCommitted {
    const ID: &'static str = "AgreementCommitted";
    type Item = ();
    type Error = CommitAgreementError;
}

/// The same messaged will be used on GSB and as messages in callbacks.
impl<Message: RpcMessage> CallbackMessage for Message {
    type Ok = <Message as RpcMessage>::Item;
    type Error = <Message as RpcMessage>::Error;
}

impl ProposalContent {
    pub fn from(proposal: DbProposal) -> ProposalContent {
        ProposalContent {
            proposal_id: proposal.id,
            properties: proposal.properties,
            constraints: proposal.constraints,
            expiration_ts: proposal.expiration_ts,
            creation_ts: proposal.creation_ts,
        }
    }
}

impl ProposalReceived {
    pub fn translate(mut self, owner: Owner) -> Self {
        self.prev_proposal_id = self.prev_proposal_id.translate(owner);
        self.proposal.proposal_id = self.proposal.proposal_id.translate(owner);
        self
    }
}

impl InitialProposalReceived {
    pub fn translate(mut self, owner: Owner) -> Self {
        self.proposal.proposal_id = self.proposal.proposal_id.translate(owner);
        self
    }
}

impl ProposalRejected {
    pub fn translate(mut self, owner: Owner) -> Self {
        self.proposal_id = self.proposal_id.translate(owner);
        self
    }
}

impl AgreementApproved {
    pub fn translate(mut self, owner: Owner) -> Self {
        self.agreement_id = self.agreement_id.translate(owner);
        self
    }
}

impl AgreementRejected {
    pub fn translate(mut self, owner: Owner) -> Self {
        self.agreement_id = self.agreement_id.translate(owner);
        self
    }
}

impl AgreementCancelled {
    pub fn translate(mut self, owner: Owner) -> Self {
        self.agreement_id = self.agreement_id.translate(owner);
        self
    }
}

impl AgreementReceived {
    pub fn translate(mut self, owner: Owner) -> Self {
        self.agreement_id = self.agreement_id.translate(owner);
        self.proposal_id = self.proposal_id.translate(owner);
        self
    }
}

impl AgreementTerminated {
    pub fn translate(mut self, owner: Owner) -> Self {
        self.agreement_id = self.agreement_id.translate(owner);
        self
    }
}

impl AgreementCommitted {
    pub fn translate(mut self, owner: Owner) -> Self {
        self.agreement_id = self.agreement_id.translate(owner);
        self
    }
}
