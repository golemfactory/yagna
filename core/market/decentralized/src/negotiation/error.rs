use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::db::model::{
    AgreementId, ProposalId, ProposalIdParseError, SubscriptionId, SubscriptionParseError,
};
use crate::db::{dao::SaveProposalError, dao::TakeEventsError, DbError};
use crate::protocol::negotiation::error::{
    AgreementError as ProtocolAgreementError, ApproveAgreementError,
    CounterProposalError as ProtocolProposalError, NegotiationApiInitError, ProposeAgreementError,
};

#[derive(Error, Debug)]
pub enum GetProposalError {
    #[error("Proposal [{0}] not found.")]
    NotFound(ProposalId),
    #[error("Failed to get Proposal [{0}]. Error: [{1}]")]
    FailedGetFromDb(ProposalId, DbError),
}

#[derive(Error, Debug)]
pub enum NegotiationError {}

#[derive(Error, Debug)]
pub enum NegotiationInitError {
    #[error("Failed to initialize Negotiation interface. Error: {0}.")]
    ApiInitError(#[from] NegotiationApiInitError),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum MatchValidationError {
    #[error("Proposal properties [{new}] doesn't match previous Proposal [{prev}].")]
    NotMatching { new: ProposalId, prev: ProposalId },
    #[error("Can't match Proposal [{new}] with previous Proposal [{prev}]. Error: {error}")]
    MatchingFailed {
        new: ProposalId,
        prev: ProposalId,
        error: String,
    },
}

#[derive(Error, Debug)]
pub enum AgreementStateError {
    #[error("Agreement [{0}] proposed.")]
    Proposed(AgreementId),
    #[error("Agreement [{0}] already confirmed.")]
    Confirmed(AgreementId),
    #[error("Agreement [{0}] cancelled.")]
    Cancelled(AgreementId),
    #[error("Agreement [{0}] rejected.")]
    Rejected(AgreementId),
    #[error("Agreement [{0}] already approved.")]
    Approved(AgreementId),
    #[error("Agreement [{0}] expired.")]
    Expired(AgreementId),
    #[error("Agreement [{0}] terminated.")]
    Terminated(AgreementId),
}

#[derive(Error, Debug)]
pub enum AgreementError {
    #[error("Agreement [{0}] not found.")]
    NotFound(AgreementId),
    #[error("Can't create Agreement for Proposal {0}. Proposal {1} not found.")]
    ProposalNotFound(ProposalId, ProposalId),
    #[error("Can't create second Agreement [{0}] for Proposal [{1}].")]
    AgreementExists(AgreementId, ProposalId),
    #[error("Can't create Agreement for Proposal {0}. Failed to get Proposal {1}. Error: {2}")]
    GetProposal(ProposalId, ProposalId, DbError),
    #[error("Can't create Agreement for already countered Proposal [{0}].")]
    ProposalCountered(ProposalId),
    #[error("Can't create Agreement for Proposal {0}. No negotiation with Provider took place. (You should counter Proposal at least one time)")]
    NoNegotiations(ProposalId),
    #[error("Can't create Agreement for out own Proposal {0}. You can promote only provider's Proposals to Agreement.")]
    OwnProposal(ProposalId),
    #[error("Failed to save Agreement for Proposal [{0}]. Error: {1}")]
    Save(ProposalId, DbError),
    #[error("Failed to get Agreement [{0}]. Error: {1}")]
    Get(AgreementId, DbError),
    #[error("Failed to update Agreement [{0}]. Error: {1}")]
    Update(AgreementId, DbError),
    #[error("Invalid state {0}")]
    InvalidState(#[from] AgreementStateError),
    #[error("Invalid Agreement id. {0}")]
    InvalidId(#[from] ProposalIdParseError),
    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolAgreementError),
    #[error("Protocol error: {0}")]
    ProtocolCreate(#[from] ProposeAgreementError),
    #[error("Protocol error while approving: {0}")]
    ProtocolApprove(#[from] ApproveAgreementError),
    #[error("Internal error: {0}")]
    InternalError(String),
}

#[derive(Error, Debug)]
pub enum WaitForApprovalError {
    #[error("Agreement [{0}] not found.")]
    NotFound(AgreementId),
    #[error("Agreement [{0}] expired.")]
    AgreementExpired(AgreementId),
    #[error("Agreement [{0}] should be confirmed, before waiting for approval.")]
    AgreementNotConfirmed(AgreementId),
    #[error("Agreement [{0}] terminated.")]
    AgreementTerminated(AgreementId),
    #[error("Timeout while waiting for Agreement [{0}] approval.")]
    Timeout(AgreementId),
    #[error("Invalid agreement id. {0}")]
    InvalidId(#[from] ProposalIdParseError),
    #[error("Failed to get Agreement [{0}]. Error: {1}")]
    FailedGetFromDb(AgreementId, DbError),
    #[error("Waiting for approval failed. Error: {0}.")]
    InternalError(String),
}

#[derive(Error, Debug)]
pub enum QueryEventsError {
    #[error("Subscription [{0}] was already unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Invalid subscription id. {0}")]
    InvalidSubscriptionId(#[from] SubscriptionParseError),
    #[error(transparent)]
    TakeEventsError(#[from] TakeEventsError),
    #[error("Invalid maxEvents '{0}', should be greater from 0.")]
    InvalidMaxEvents(i32),
    #[error("Can't query events. Error: {0}.")]
    InternalError(String),
}

#[derive(Error, Debug)]
pub enum ProposalError {
    #[error("Subscription [{0}] wasn't found.")]
    NoSubscription(SubscriptionId),
    #[error("Subscription [{0}] was already unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Subscription [{0}] expired.")]
    SubscriptionExpired(SubscriptionId),
    #[error("Proposal [{0}] not found for subscription [{1}].")]
    ProposalNotFound(ProposalId, SubscriptionId),
    #[error("Proposal [{0}] was already countered. Can't counter for the second time.")]
    AlreadyCountered(ProposalId),
    #[error("Can't counter own Proposal [{0}].")]
    OwnProposal(ProposalId),
    #[error(transparent)]
    NotMatching(#[from] MatchValidationError),
    #[error("Failed to get Proposal [{0}] for subscription [{1}]. Error: [{2}]")]
    FailedGetProposal(ProposalId, SubscriptionId, DbError),
    #[error("Failed to save counter Proposal for Proposal [{0}]. Error: {1}")]
    FailedSaveProposal(ProposalId, SaveProposalError),
    #[error("Failed to send counter Proposal for Proposal [{0}]. Error: {1}")]
    FailedSendProposal(ProposalId, ProtocolProposalError),
    #[error("Can't counter Proposal [{0}]. Error: {1}.")]
    InternalError(ProposalId, String),
}

impl AgreementError {
    pub fn from(promoted_proposal: &ProposalId, e: GetProposalError) -> AgreementError {
        match e {
            GetProposalError::NotFound(id) => {
                AgreementError::ProposalNotFound(promoted_proposal.clone(), id)
            }
            GetProposalError::FailedGetFromDb(id, db_error) => {
                AgreementError::GetProposal(promoted_proposal.clone(), id, db_error)
            }
        }
    }
}

impl ProposalError {
    pub fn from(subscription_id: &SubscriptionId, e: GetProposalError) -> ProposalError {
        match e {
            GetProposalError::NotFound(id) => {
                ProposalError::ProposalNotFound(id, subscription_id.clone())
            }
            GetProposalError::FailedGetFromDb(id, db_error) => {
                ProposalError::FailedGetProposal(id, subscription_id.clone(), db_error)
            }
        }
    }
}
