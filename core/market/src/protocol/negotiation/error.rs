use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::db::model::{
    AgreementId, AgreementState, ProposalId, ProposalIdValidationError, SubscriptionId,
};
use crate::negotiation::error::MatchValidationError;

/// Trait for Error types, that shouldn't expose sensitive information
/// to other Nodes in network, but should contain more useful message, when displaying
/// them on local Node.
pub trait RemoteSensitiveError {
    fn hide_sensitive_info(self) -> RemoteProposeAgreementError;
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum NegotiationApiInitError {}

#[derive(Error, Debug, Serialize, Deserialize)]
#[error("Proposal [{1}] GSB error: {0}.")]
pub struct GsbProposalError(pub String, pub ProposalId);

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum CounterProposalError {
    #[error("Countering {0}.")]
    Gsb(#[from] GsbProposalError),
    #[error("Countering Proposal [{0}] without previous Proposal id set.")]
    NoPrevious(ProposalId),
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
    NotFound(ProposalId),
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
#[error("Agreement [{1}] GSB error: {0}.")]
pub struct GsbAgreementError(pub String, pub AgreementId);

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ProposeAgreementError {
    #[error("Propose {0}.")]
    Gsb(#[from] GsbAgreementError),
    #[error("Agreement [{1}] remote error: {0}")]
    Remote(RemoteProposeAgreementError, AgreementId),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum RemoteProposeAgreementError {
    #[error("Proposal [{0}] not found.")]
    NotFound(ProposalId),
    #[error("Requestor can't promote his own Proposal [{0}] to Agreement.")]
    RequestorOwn(ProposalId),
    #[error("Can't create Agreement for Proposal {0}. No negotiation with Provider took place. (You should counter Proposal at least one time)")]
    NoNegotiations(ProposalId),
    #[error("Can't create Agreement for already countered Proposal [{0}].")]
    AlreadyCountered(ProposalId),
    #[error("Agreement id [{0}] is invalid.")]
    InvalidId(AgreementId),
    /// We should hide `original_msg`, since we don't want to reveal our details to
    /// other Nodes. On the other side we should log whole message on local Node.
    /// Use `RemoteSensitiveError::hide_sensitive_info` for this.
    #[error("Unexpected error: {public_msg} {original_msg}.")]
    Unexpected {
        public_msg: String,
        original_msg: String,
    },
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ApproveAgreementError {
    #[error("Approve {0}.")]
    Gsb(#[from] GsbAgreementError),
    #[error("Remote failed to approve. Error: {0}")]
    Remote(RemoteAgreementError),
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

impl RemoteSensitiveError for RemoteProposeAgreementError {
    fn hide_sensitive_info(self) -> RemoteProposeAgreementError {
        match self {
            RemoteProposeAgreementError::Unexpected { public_msg, .. } => {
                RemoteProposeAgreementError::Unexpected {
                    public_msg,
                    original_msg: "".to_string(),
                }
            }
            _ => self,
        }
    }
}
