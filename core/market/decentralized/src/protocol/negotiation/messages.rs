use serde::{Deserialize, Serialize};

use super::errors::ProposalError;
use super::super::callbacks::CallbackMessage;


#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalReceived {

}

impl CallbackMessage for ProposalReceived {
    type Item = ();
    type Error = ProposalError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialProposalReceived {

}

impl CallbackMessage for InitialProposalReceived {
    type Item = ();
    type Error = ProposalError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalRejected {

}

impl CallbackMessage for ProposalRejected {
    type Item = ();
    type Error = ProposalError;
}

