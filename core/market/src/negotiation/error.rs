use serde::{Deserialize, Serialize};
use thiserror::Error;

use ya_client::model::NodeId;

use crate::db::dao::AgreementDaoError;
use crate::db::model::{
    AgreementId, ProposalId, ProposalIdParseError, SubscriptionId, SubscriptionParseError,
};
use crate::db::{
    dao::TakeEventsError,
    dao::{ChangeProposalStateError, SaveProposalError},
    DbError,
};
use crate::matcher::error::{DemandError, QueryOfferError};
use crate::protocol::negotiation::error::{
    AgreementProtocolError, CommitAgreementError, CounterProposalError as ProtocolProposalError,
    GsbAgreementError, NegotiationApiInitError, ProposeAgreementError, RejectProposalError,
    TerminateAgreementError,
};

#[derive(Error, Debug)]
pub enum NegotiationError {}

#[derive(Error, Debug)]
#[error("Failed to initialize Negotiation interface. Error: {0}.")]
pub struct NegotiationInitError(#[from] NegotiationApiInitError);

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum MatchValidationError {
    #[error("Proposal properties [{new}] doesn't match previous Proposal [{prev}]. {mismatches}")]
    NotMatching {
        new: ProposalId,
        prev: ProposalId,
        mismatches: String,
    },
    #[error("Can't match Proposal [{new}] with previous Proposal [{prev}]. Error: {error}")]
    MatchingFailed {
        new: ProposalId,
        prev: ProposalId,
        error: String,
    },
}

#[derive(Error, Debug)]
pub enum AgreementError {
    #[error("Agreement [{0}] not found.")]
    NotFound(String),
    #[error("Agreement [{0}] expired.")]
    Expired(AgreementId),
    #[error("Can't create Agreement for Proposal [{0}]. Proposal [{1}] not found.")]
    ProposalNotFound(ProposalId, ProposalId),
    #[error("Can't create second Agreement for Proposal [{0}].")]
    ProposalAlreadyAccepted(ProposalId),
    #[error("Can't create Agreement for Proposal [{0}]. Failed to get Proposal [{1}]. Error: {2}")]
    GetProposal(ProposalId, ProposalId, String),
    #[error("Can't create Agreement for already countered Proposal [{0}].")]
    ProposalCountered(ProposalId),
    #[error("Can't create Agreement for Proposal [{0}]. No negotiation with Provider took place. (You should counter Proposal at least one time)")]
    NoNegotiations(ProposalId),
    #[error("Can't create Agreement for an own Proposal [{0}]. You can promote only provider's Proposals to Agreement.")]
    OwnProposal(ProposalId),
    #[error("Can't create Agreement for rejected Proposal [{0}].")]
    ProposalRejected(ProposalId),
    #[error("Failed to save Agreement for Proposal [{0}]. Error: {1}")]
    Save(ProposalId, DbError),
    #[error("Failed to get Agreement [{0}]. Error: {1}")]
    Get(String, AgreementDaoError),
    #[error("Agreement [{0}]. Error: {1}")]
    UpdateState(AgreementId, AgreementDaoError),
    #[error("Invalid Agreement id. {0}")]
    InvalidId(#[from] ProposalIdParseError),
    #[error(transparent)]
    Gsb(#[from] GsbAgreementError),
    #[error("Protocol error: {0}")]
    ProtocolCreate(#[from] ProposeAgreementError),
    #[error("Protocol error while approving: {0}")]
    Protocol(#[from] AgreementProtocolError),
    #[error("Protocol error while terminating: {0}")]
    ProtocolTerminate(#[from] TerminateAgreementError),
    #[error("Protocol error while committing: {0}")]
    ProtocolCommit(#[from] CommitAgreementError),
    #[error("Internal error: {0}")]
    Internal(String),
}

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
    Get(AgreementId, AgreementDaoError),
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

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum GetProposalError {
    #[error("Proposal [{0}] not found (subscription [{1:?}]).")]
    NotFound(ProposalId, Option<SubscriptionId>),
    #[error("Get Proposal [{0}] (subscription [{1:?}]) internal error: [{2}]")]
    Internal(ProposalId, Option<SubscriptionId>, String),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ProposalValidationError {
    #[error("Subscription [{0}] wasn't found.")]
    NoSubscription(SubscriptionId),
    #[error("Subscription [{0}] was already unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Subscription [{0}] expired.")]
    SubscriptionExpired(SubscriptionId),
    #[error(transparent)]
    NotMatching(#[from] MatchValidationError),
    #[error("Can't react to own Proposal [{0}].")]
    OwnProposal(ProposalId),
    #[error("Unauthorized operation attempt on Proposal [{0}] from [{1}].")]
    Unauthorized(ProposalId, NodeId),
    #[error("Internal error processing Proposal: {0}.")]
    Internal(String),
}

#[derive(Error, Debug)]
pub enum ProposalError {
    #[error(transparent)]
    Validation(#[from] ProposalValidationError),
    #[error(transparent)]
    Get(#[from] GetProposalError),
    #[error(transparent)]
    JsonObjectExpected(#[from] serde_json::error::Error),
    #[error(transparent)]
    Save(#[from] SaveProposalError),
    #[error(transparent)]
    ChangeState(#[from] ChangeProposalStateError),
    #[error(transparent)]
    Reject(#[from] RejectProposalError),
    #[error("Failed to send response for Proposal [{0}]. Error: {1}")]
    Send(ProposalId, ProtocolProposalError),
}

#[derive(Error, Debug)]
#[error("Failed regenerate proposal: {0}.")]
pub enum RegenerateProposalError {
    Offer(#[from] QueryOfferError),
    Demand(#[from] DemandError),
    Save(#[from] SaveProposalError),
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

impl From<MatchValidationError> for ProposalError {
    fn from(e: MatchValidationError) -> Self {
        ProposalValidationError::NotMatching(e).into()
    }
}

impl From<QueryOfferError> for ProposalValidationError {
    fn from(e: QueryOfferError) -> Self {
        match e {
            QueryOfferError::NotFound(id) => ProposalValidationError::NoSubscription(id),
            QueryOfferError::Unsubscribed(id) => ProposalValidationError::Unsubscribed(id),
            QueryOfferError::Expired(id) => ProposalValidationError::SubscriptionExpired(id),
            _ => ProposalValidationError::Internal(format!("Offer: {}", e)),
        }
    }
}

impl From<DemandError> for ProposalValidationError {
    fn from(e: DemandError) -> Self {
        match e {
            DemandError::NotFound(id) => ProposalValidationError::NoSubscription(id),
            _ => ProposalValidationError::Internal(format!("Demand: {}", e)),
        }
    }
}
