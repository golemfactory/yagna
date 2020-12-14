use serde::{Deserialize, Serialize};
use thiserror::Error;

use ya_market_resolver::flatten::JsonObjectExpected;

use crate::db::dao::StateError;
use crate::db::model::{
    AgreementId, ProposalId, ProposalIdParseError, SubscriptionId, SubscriptionParseError,
};
use crate::db::{dao::SaveProposalError, dao::TakeEventsError, DbError};
use crate::protocol::negotiation::error::{
    ApproveAgreementError, CounterProposalError as ProtocolProposalError, GsbAgreementError,
    NegotiationApiInitError, ProposeAgreementError, TerminateAgreementError,
};

#[derive(Error, Debug)]
pub enum NegotiationError {}

#[derive(Error, Debug)]
#[error("Failed to initialize Negotiation interface. Error: {0}.")]
pub struct NegotiationInitError(#[from] NegotiationApiInitError);

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
    AlreadyExists(AgreementId, ProposalId),
    #[error("Can't create Agreement for Proposal {0}. Failed to get Proposal {1}. Error: {2}")]
    GetProposal(ProposalId, ProposalId, String),
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
    #[error("Agreement [{0}]. Error: {1}")]
    UpdateState(AgreementId, StateError),
    #[error("Invalid state. {0}")]
    InvalidState(#[from] AgreementStateError),
    #[error("Invalid Agreement id. {0}")]
    InvalidId(#[from] ProposalIdParseError),
    #[error(transparent)]
    Gsb(#[from] GsbAgreementError),
    #[error("Protocol error: {0}")]
    ProtocolCreate(#[from] ProposeAgreementError),
    #[error("Protocol error while approving: {0}")]
    ProtocolApprove(#[from] ApproveAgreementError),
    #[error("Protocol error while terminating: {0}")]
    ProtocolTerminate(#[from] TerminateAgreementError),
    #[error(transparent)]
    ReasonError(#[from] ReasonError),
    #[error("Internal error: {0}")]
    Internal(String),
}

#[derive(Error, Debug)]
#[error("Reason parse error: {0}")]
pub struct ReasonError(#[from] pub serde_json::Error);

#[derive(Error, Debug)]
pub enum WaitForApprovalError {
    #[error("Agreement [{0}] not found.")]
    NotFound(AgreementId),
    #[error("Agreement [{0}] expired.")]
    Expired(AgreementId),
    #[error("Agreement [{0}] should be confirmed, before waiting for approval.")]
    NotConfirmed(AgreementId),
    #[error("Agreement [{0}] terminated.")]
    Terminated(AgreementId),
    #[error("Timeout while waiting for Agreement [{0}] approval.")]
    Timeout(AgreementId),
    #[error("Invalid agreement id. {0}")]
    InvalidId(#[from] ProposalIdParseError),
    #[error("Failed to get Agreement [{0}]. Error: {1}")]
    Get(AgreementId, DbError),
    #[error("Waiting for approval failed. Error: {0}.")]
    Internal(String),
}

#[derive(Error, Debug)]
pub enum QueryEventsError {
    #[error("Invalid subscription id. {0}")]
    InvalidSubscriptionId(#[from] SubscriptionParseError),
    #[error(transparent)]
    TakeEvents(#[from] TakeEventsError),
    #[error("Invalid maxEvents '{0}', should be between 1 and {1}.")]
    InvalidMaxEvents(i32, i32),
    #[error("Can't query events. Error: {0}.")]
    Internal(String),
}

#[derive(Error, Debug)]
pub enum AgreementEventsError {
    #[error("Invalid maxEvents '{0}', should be between 1 and {1}.")]
    InvalidMaxEvents(i32, i32),
    #[error("Internal error while querying Agreement events. Error: {0}.")]
    Internal(String),
}

#[derive(Error, Debug)]
pub enum GetProposalError {
    #[error("Proposal [{0}] not found (subscription [{1:?}]).")]
    NotFound(ProposalId, Option<SubscriptionId>),
    #[error("Get Proposal [{0}] (subscription [{1:?}]) internal error: [{2}]")]
    Internal(ProposalId, Option<SubscriptionId>, String),
}

#[derive(Error, Debug)]
pub enum ProposalError {
    #[error("Subscription [{0}] wasn't found.")]
    NoSubscription(SubscriptionId),
    #[error("Subscription [{0}] was already unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Subscription [{0}] expired.")]
    SubscriptionExpired(SubscriptionId),
    #[error("Can't counter own Proposal [{0}].")]
    OwnProposal(ProposalId),
    #[error(transparent)]
    NotMatching(#[from] MatchValidationError),
    #[error(transparent)]
    Get(#[from] GetProposalError),
    #[error(transparent)]
    JsonObjectExpected(#[from] JsonObjectExpected),
    #[error(transparent)]
    Save(#[from] SaveProposalError),
    #[error("Failed to send counter Proposal for Proposal [{0}]. Error: {1}")]
    Send(ProposalId, ProtocolProposalError),
    #[error("Can't counter Proposal [{0}]. Error: {1}.")]
    Internal(ProposalId, String),
}

impl AgreementError {
    pub fn from_proposal(promoted_proposal: &ProposalId, e: GetProposalError) -> AgreementError {
        match e {
            GetProposalError::NotFound(proposal_id, ..) => {
                AgreementError::ProposalNotFound(promoted_proposal.clone(), proposal_id)
            }
            GetProposalError::Internal(proposal_id, _, err_msg) => {
                AgreementError::GetProposal(promoted_proposal.clone(), proposal_id, err_msg)
            }
        }
    }
}
