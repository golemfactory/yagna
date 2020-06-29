use serde::{Deserialize, Serialize};

use super::super::callbacks::CallbackMessage;
use super::errors::{AgreementError, ProposalError};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalReceived {}

impl CallbackMessage for ProposalReceived {
    type Item = ();
    type Error = ProposalError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialProposalReceived {}

impl CallbackMessage for InitialProposalReceived {
    type Item = ();
    type Error = ProposalError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalRejected {
    pub proposal_id: String,
}

impl CallbackMessage for ProposalRejected {
    type Item = ();
    type Error = ProposalError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementReceived {}

impl CallbackMessage for AgreementReceived {
    type Item = ();
    type Error = AgreementError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementApproved {}

impl CallbackMessage for AgreementApproved {
    type Item = ();
    type Error = AgreementError;
}

/// Agreement was cancelled or rejected.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreementRejected {
    pub agreement_id: String,
}

impl CallbackMessage for AgreementRejected {
    type Item = ();
    type Error = AgreementError;
}
