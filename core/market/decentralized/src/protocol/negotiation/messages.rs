use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use super::super::callbacks::CallbackMessage;
use super::errors::{AgreementError, CounterProposalError, ProposalError};
use crate::db::models::DbProposal;
use crate::db::models::Demand;
use crate::SubscriptionId;

use ya_client::model::NodeId;
use ya_service_bus::RpcMessage;

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
    pub proposal_id: String,
    pub properties: String,
    pub constraints: String,

    pub creation_ts: NaiveDateTime,
    pub expiration_ts: NaiveDateTime,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalReceived {
    pub prev_proposal_id: String,
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
    pub proposal_id: String,
}

impl RpcMessage for ProposalRejected {
    const ID: &'static str = "ProposalRejected";
    type Item = ();
    type Error = ProposalError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementReceived {
    pub agreement_id: String,
    // TODO: Send agreement content.
}

impl RpcMessage for AgreementReceived {
    const ID: &'static str = "AgreementReceived";
    type Item = ();
    type Error = AgreementError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementApproved {
    pub agreement_id: String,
    // TODO: We should send here signature.
}

impl RpcMessage for AgreementApproved {
    const ID: &'static str = "AgreementApproved";
    type Item = ();
    type Error = AgreementError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementRejected {
    pub agreement_id: String,
}

impl RpcMessage for AgreementRejected {
    const ID: &'static str = "AgreementRejected";
    type Item = ();
    type Error = AgreementError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementCancelled {
    pub agreement_id: String,
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
            proposal_id: proposal.id.to_string(),
            properties: proposal.properties,
            constraints: proposal.constraints,
            expiration_ts: proposal.expiration_ts,
            creation_ts: proposal.creation_ts,
        }
    }
}

impl InitialProposalReceived {
    pub fn into_demand(self, owner: NodeId) -> Demand {
        Demand {
            id: self.demand_id,
            properties: self.proposal.properties,
            constraints: self.proposal.constraints,
            node_id: owner,
            creation_ts: self.proposal.creation_ts,
            insertion_ts: None,
            expiration_ts: self.proposal.expiration_ts,
        }
    }
}
