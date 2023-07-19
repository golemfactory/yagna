use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::db::dao::ChangeProposalStateError;
use crate::db::model::{AgreementId, AgreementState, ProposalId, ProposalIdValidationError};
use crate::matcher::error::QueryOfferError;
use crate::negotiation::error::{GetProposalError, MatchValidationError, ProposalValidationError};

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
    #[error("Remote error: {0}")]
    RemoteInternal(#[from] RemoteProposalError),
    #[error(transparent)]
    CallerParse(#[from] CallerParseError),
    #[error("Timeout while sending counter Proposal")]
    Timeout,
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum RejectProposalError {
    #[error("Rejecting {0}.")]
    Gsb(#[from] GsbProposalError),
    #[error(transparent)]
    Get(#[from] GetProposalError),
    #[error(transparent)]
    ChangeState(#[from] ChangeProposalStateError),
    #[error(transparent)]
    Validation(#[from] ProposalValidationError),
    #[error(transparent)]
    CallerParse(#[from] CallerParseError),
}

#[derive(Error, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RemoteProposalError {
    #[error(transparent)]
    Validation(#[from] ProposalValidationError),
    #[error("Trying to counter not existing Proposal [{0}].")]
    NotFound(ProposalId),
    #[error("Proposal [{0}] was already countered.")]
    AlreadyCountered(ProposalId),
    #[error(transparent)]
    InvalidId(#[from] ProposalIdValidationError),
    #[error(transparent)]
    CallerParse(#[from] CallerParseError),
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
    #[error("Agreement [{0}] not signed.")]
    NotSigned(AgreementId),
}

#[derive(Error, Debug, Serialize, Deserialize)]
#[non_exhaustive]
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
pub enum AgreementProtocolError {
    #[error("Approve {0}.")]
    Gsb(#[from] GsbAgreementError),
    #[error("Remote failed to approve. Error: {0}")]
    Remote(RemoteAgreementError),
    #[error(transparent)]
    CallerParse(#[from] CallerParseError),
    #[error("Timeout while sending approval of Agreement [{0}]")]
    Timeout(AgreementId),
    #[error("Agreement [{0}] doesn't contain approval timestamp.")]
    NoApprovalTimestamp(AgreementId),
    #[error("Agreement [{0}] not signed.")]
    NotSigned(AgreementId),
}

#[derive(Error, Debug, Serialize, Deserialize)]
#[error("Failed to parse caller {caller}: {e}")]
pub struct CallerParseError {
    pub caller: String,
    pub e: String,
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum TerminateAgreementError {
    #[error("Terminate {0}.")]
    Gsb(#[from] GsbAgreementError),
    #[error("Remote Terminate: {0}")]
    Remote(#[from] RemoteAgreementError),
    #[error(transparent)]
    CallerParse(#[from] CallerParseError),
}

#[derive(Error, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RemoteAgreementError {
    #[error("Agreement [{0}] not found.")]
    NotFound(AgreementId),
    #[error("Agreement [{0}] expired.")]
    Expired(AgreementId),
    #[error("Agreement [{0}] in state {1}, can't be approved.")]
    InvalidState(AgreementId, AgreementState),
    #[error("Can't finish operation on Agreement [{0}] due to internal error.")]
    InternalError(AgreementId),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum CommitAgreementError {
    #[error("Commit Agreement {0}.")]
    Gsb(#[from] GsbAgreementError),
    #[error("Remote commit Agreement [{1}] error: {0}")]
    Remote(RemoteCommitAgreementError, AgreementId),
    #[error(transparent)]
    CallerParse(#[from] CallerParseError),
    #[error("Agreement [{0}] not signed.")]
    NotSigned(AgreementId),
}

#[derive(Error, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RemoteCommitAgreementError {
    #[error("Agreement expired.")]
    Expired,
    #[error("Agreement cancelled.")]
    Cancelled,
    #[error("Agreement not found.")]
    NotFound,
    #[error("Agreement in state {0}, can't be committed.")]
    InvalidState(AgreementState),
    #[error("Unexpected error: {public_msg} {original_msg}.")]
    Unexpected {
        public_msg: String,
        original_msg: String,
    },
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

impl From<MatchValidationError> for RemoteProposalError {
    fn from(e: MatchValidationError) -> Self {
        ProposalValidationError::NotMatching(e).into()
    }
}

impl From<QueryOfferError> for RemoteProposalError {
    fn from(e: QueryOfferError) -> Self {
        ProposalValidationError::from(e).into()
    }
}

impl From<ProposalValidationError> for CounterProposalError {
    fn from(e: ProposalValidationError) -> Self {
        RemoteProposalError::from(e).into()
    }
}
