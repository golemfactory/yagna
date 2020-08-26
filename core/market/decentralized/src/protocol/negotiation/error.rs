use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::db::model::{
    AgreementId, AgreementState, ProposalId, ProposalIdValidationError, SubscriptionId,
};
use crate::negotiation::error::MatchValidationError;

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
    #[error("Proposal [{0}] was already countered.")]
    AlreadyCountered(ProposalId),
    #[error(transparent)]
    InvalidId(#[from] ProposalIdValidationError),
    #[error(transparent)]
    NotMatching(#[from] MatchValidationError),
    #[error("Error: {0}.")]
    Unexpected(String),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum AgreementError {
    #[error("Agreement [{1}] GSB error: {0}.")]
    GsbError(String, AgreementId),
    #[error("Saving Agreement [{1}] error: {0}.")]
    Saving(String, AgreementId),
    #[error("Agreement [{1}] remote error: {0}")]
    Remote(RemoteAgreementError, AgreementId),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ProposeAgreementError {
    #[error("Agreement [{1}] GSB error: {0}.")]
    GsbError(String, AgreementId),
    #[error("Agreement [{1}] remote error: {0}")]
    Remote(RemoteProposeAgreementError, AgreementId),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum RemoteProposeAgreementError {
    #[error("Proposal [{0}] not found.")]
    ProposalNotFound(ProposalId),
    #[error("Requestor can't promote his own Proposal [{0}] to Agreement.")]
    RequestorProposal(ProposalId),
    #[error("Can't create Agreement for Proposal {0}. No negotiation with Provider took place. (You should counter Proposal at least one time)")]
    NoNegotiations(ProposalId),
    #[error("Can't create Agreement for already countered Proposal [{0}].")]
    ProposalCountered(ProposalId),
    #[error("Agreement id [{0}] is invalid.")]
    InvalidId(AgreementId),
    #[error("Unexpected error: {0}.")]
    Unexpected(String),
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
