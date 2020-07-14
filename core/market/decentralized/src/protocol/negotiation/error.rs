use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::db::model::{SubscriptionId, AgreementId, AgreementState, ProposalId};

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum NegotiationApiInitError {}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ProposalError {
    #[error("Failed to broadcast caused by gsb error: {0}.")]
    GsbError(String),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum CounterProposalError {
    #[error("Failed to broadcast caused by gsb error: {0}.")]
    GsbError(String),
    #[error("Trying to counter Proposal [{0}] without previous Proposal id set.")]
    NoPreviousProposal(ProposalId),
    #[error("Can't counter proposal due to remote node error: {0}")]
    Remote(#[from] RemoteProposalError),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum RemoteProposalError {
    #[error("Offer/Demand [{0}] already unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Offer/Demand [{0}] expired.")]
    Expired(SubscriptionId),
    #[error("Trying to counter not existing Proposal [{0}].")]
    ProposalNotFound(ProposalId),
    #[error("Error: {0}.")]
    Unexpected(String),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum AgreementError {
    #[error("Failed to broadcast caused by gsb error: {0}.")]
    GsbError(String),
    #[error("Saving Agreement [{1}] error: {0}.")]
    Saving(String, AgreementId),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ApproveAgreementError {
    #[error("Failed to broadcast caused by gsb error: {0}.")]
    GsbError(String),
    #[error("Can't approve Agreement due to remote node error: {0}")]
    Remote(#[from] RemoteAgreementError),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum RemoteAgreementError {
    #[error("Agreement {0} not found.")]
    NotFound(AgreementId),
    #[error("Agreement {0} expired.")]
    Expired(AgreementId),
    #[error("Agreement {0} in state {1}, can't be approved.")]
    InvalidState(AgreementId, AgreementState),
    #[error("Can't approve Agreement {0} due to internal error.")]
    InternalError(AgreementId),
}

impl From<ya_service_bus::error::Error> for ProposalError {
    fn from(e: ya_service_bus::error::Error) -> Self {
        ProposalError::GsbError(e.to_string())
    }
}

impl From<ya_service_bus::error::Error> for CounterProposalError {
    fn from(e: ya_service_bus::error::Error) -> Self {
        CounterProposalError::GsbError(e.to_string())
    }
}

impl From<ya_service_bus::error::Error> for AgreementError {
    fn from(e: ya_service_bus::error::Error) -> Self {
        AgreementError::GsbError(e.to_string())
    }
}

impl From<ya_service_bus::error::Error> for ApproveAgreementError {
    fn from(e: ya_service_bus::error::Error) -> Self {
        ApproveAgreementError::GsbError(e.to_string())
    }
}
