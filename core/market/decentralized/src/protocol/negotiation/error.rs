use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::db::model::{AgreementId, AgreementState, ProposalId, SubscriptionId};

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum NegotiationApiInitError {}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ProposalError {
    #[error("Proposal [{1}] GSB error: {0}.")]
    GsbError(String, ProposalId),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum CounterProposalError {
    #[error("Countering Proposal [{1}] GSB error: {0}.")]
    GsbError(String, ProposalId),
    #[error("Countering Proposal [{0}] without previous Proposal id set.")]
    NoPreviousProposal(ProposalId),
    #[error("Countering Proposal [{1}] remote error: {0}")]
    Remote(RemoteProposalError, ProposalId),
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
    #[error("Agreement [{1}] GSB error: {0}.")]
    GsbError(String, AgreementId),
    #[error("Saving Agreement [{1}] error: {0}.")]
    Saving(String, AgreementId),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ApproveAgreementError {
    #[error("Approving Agreement [{1}] GSB error: {0}.")]
    GsbError(String, AgreementId),
    #[error("Approving Agreement [{1}] remote error: {0}")]
    Remote(RemoteAgreementError, AgreementId),
    #[error("Can't parse {caller} for Agreement [{id}]: {e}")]
    CallerParseError {
        e: String,
        caller: String,
        id: AgreementId,
    },
    #[error("Timeout while sending approval of Agreement [{0}]")]
    Timeout(AgreementId),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum RemoteAgreementError {
    #[error("Agreement [{0}] not found.")]
    NotFound(AgreementId),
    #[error("Agreement [{0}] expired.")]
    Expired(AgreementId),
    #[error("Agreement [{0}] in state {1}, can't be approved.")]
    InvalidState(AgreementId, AgreementState),
    #[error("Can't approve Agreement [{0}] due to internal error.")]
    InternalError(AgreementId),
}
