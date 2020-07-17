use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use ya_service_bus::RpcMessage;

use crate::db::model::{Agreement, AgreementId};
use crate::db::model::{DbProposal, OwnerType, ProposalId, SubscriptionId};

use super::super::callback::CallbackMessage;
use super::error::{AgreementError, ApproveAgreementError, CounterProposalError, ProposalError};

pub mod provider {
    pub fn proposal_addr(prefix: &str) -> String {
        format!("{}/protocol/negotiation/provider/proposal", prefix)
    }

    pub fn agreement_addr(prefix: &str) -> String {
        format!("{}/protocol/negotiation/provider/agreement", prefix)
    }
}

pub mod requestor {
    pub fn proposal_addr(prefix: &str) -> String {
        format!("{}/protocol/negotiation/requestor/proposal", prefix)
    }

    pub fn agreement_addr(prefix: &str) -> String {
        format!("{}/protocol/negotiation/requestor/agreement", prefix)
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalRejected {
    pub proposal_id: ProposalId,
}

impl RpcMessage for ProposalRejected {
    const ID: &'static str = "ProposalRejected";
    type Item = ();
    type Error = ProposalError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementReceived {
    pub agreement: Agreement,
}

impl RpcMessage for AgreementReceived {
    const ID: &'static str = "AgreementReceived";
    type Item = ();
    type Error = AgreementError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementApproved {
    pub agreement_id: AgreementId,
    // TODO: We should send here signature.
}

impl RpcMessage for AgreementApproved {
    const ID: &'static str = "AgreementApproved";
    type Item = ();
    type Error = ApproveAgreementError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementRejected {
    pub agreement_id: AgreementId,
}

impl RpcMessage for AgreementRejected {
    const ID: &'static str = "AgreementRejected";
    type Item = ();
    type Error = AgreementError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementCancelled {
    pub agreement_id: AgreementId,
}

impl RpcMessage for AgreementCancelled {
    const ID: &'static str = "AgreementCancelled";
    type Item = ();
    type Error = AgreementError;
}

/// The same messaged will be used on GSB and as messages in callbacks.
impl<Message: RpcMessage> CallbackMessage for Message {
    type Item = <Message as RpcMessage>::Item;
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
    pub fn translate(mut self, owner: OwnerType) -> Self {
        self.prev_proposal_id = self.prev_proposal_id.translate(owner.clone());
        self.proposal.proposal_id = self.proposal.proposal_id.translate(owner);
        self
    }
}

impl InitialProposalReceived {
    pub fn translate(mut self, owner: OwnerType) -> Self {
        self.proposal.proposal_id = self.proposal.proposal_id.translate(owner);
        self
    }
}

impl ProposalRejected {
    pub fn translate(mut self, owner: OwnerType) -> Self {
        self.proposal_id = self.proposal_id.translate(owner);
        self
    }
}

impl AgreementApproved {
    pub fn translate(mut self, owner: OwnerType) -> Self {
        self.agreement_id = self.agreement_id.translate(owner.clone());
        self
    }
}
