use serde::{Deserialize, Serialize};

use super::super::callbacks::CallbackMessage;
use super::errors::{AgreementError, ProposalError};

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
#[serde(rename_all = "camelCase")]
pub struct ProposalReceived {
    pub proposal_id: String,
    // TODO: We should send Demand part of the proposal.
}

impl RpcMessage for ProposalReceived {
    const ID: &'static str = "ProposalReceived";
    type Item = ();
    type Error = ProposalError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialProposalReceived {
    pub proposal_id: String,
    // TODO: We should send Requestor Demand and proposal id.
}

impl RpcMessage for InitialProposalReceived {
    const ID: &'static str = "InitialProposalReceived";
    type Item = ();
    type Error = ProposalError;
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

/// Agreement was cancelled or rejected.
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

/// The same messaged will be used on GSB and as messages in callbacks.
impl<Message: RpcMessage> CallbackMessage for Message {
    type Item = <Message as RpcMessage>::Item;
    type Error = <Message as RpcMessage>::Error;
}
