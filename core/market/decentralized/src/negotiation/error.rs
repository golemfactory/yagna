use thiserror::Error;

use ya_client::model::ErrorMessage;

use crate::db::model::{
    AgreementId, ProposalId, ProposalIdParseError, SubscriptionId, SubscriptionParseError,
};
use crate::db::{dao::TakeEventsError, DbError};
use crate::matcher::error::{DemandError, QueryOfferError};
use crate::protocol::negotiation::error::{
    AgreementError as ProtocolAgreementError, ApproveAgreementError,
    CounterProposalError as ProtocolProposalError, NegotiationApiInitError,
};

#[derive(Error, Debug)]
pub enum NegotiationError {}

#[derive(Error, Debug)]
pub enum NegotiationInitError {
    #[error("Failed to initialize Negotiation interface. Error: {0}.")]
    ApiInitError(#[from] NegotiationApiInitError),
}

#[derive(Error, Debug)]
pub enum AgreementError {
    #[error("Can't create Agreement for Proposal {0}. Proposal {1} not found.")]
    ProposalNotFound(ProposalId, ProposalId),
    #[error("Can't create Agreement for Proposal {0}. Failed to get Proposal {1}. Error: {2}")]
    GetProposal(ProposalId, ProposalId, DbError),
    #[error("Can't create Agreement for Proposal {0}. No negotiation with Provider took place. (You should counter Proposal at least one time)")]
    NoNegotiations(ProposalId),
    #[error("Failed to save Agreement for Proposal [{0}]. Error: {1}")]
    Save(ProposalId, DbError),
    #[error("Failed to get Agreement [{0}]. Error: {1}")]
    Get(AgreementId, DbError),
    #[error("Failed to update Agreement [{0}]. Error: {1}")]
    Update(AgreementId, DbError),
    #[error("Agreement [{0}] not found.")]
    NotFound(AgreementId),
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
    #[error("Invalid proposal id. {0}")]
    InvalidSubscriptionId(#[from] ProposalIdParseError),
    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolAgreementError),
    #[error("Protocol error while approving: {0}")]
    ProtocolApprove(#[from] ApproveAgreementError),
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
    #[error("Failed to get Agreement [{0}]. Error: {1}")]
    Get(AgreementId, DbError),
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
    #[error("Proposal subscription Offer error: [{0}].")]
    QueryOfferError(#[from] QueryOfferError),
    #[error("Proposal subscription Demand error: [{0}].")]
    DemandError(#[from] DemandError),
    #[error("Subscription [{0}] expired.")]
    SubscriptionExpired(SubscriptionId),
    #[error("Proposal [{0}] not found for subscription [{1:?}].")]
    NotFound(ProposalId, Option<SubscriptionId>),
    #[error("Failed to get Proposal [{0}] for subscription [{1:?}]. Error: [{2}]")]
    Get(ProposalId, Option<SubscriptionId>, DbError),
    #[error("Failed to save counter Proposal for Proposal [{0}]. Error: {1}")]
    Save(ProposalId, DbError),
    #[error("Failed to send counter Proposal for Proposal [{0}]. Error: {1}")]
    Send(ProposalId, ProtocolProposalError),
    #[error("Internal error: {0}.")]
    InternalError(#[from] ErrorMessage),
}

impl AgreementError {
    pub fn from(promoted_proposal: &ProposalId, e: ProposalError) -> AgreementError {
        match e {
            ProposalError::NotFound(proposal_id, ..) => {
                AgreementError::ProposalNotFound(promoted_proposal.clone(), proposal_id)
            }
            ProposalError::Get(proposal_id, _, db_error) => {
                AgreementError::GetProposal(promoted_proposal.clone(), proposal_id, db_error)
            }
            _ => panic!("invalid conversion from {:?} to AgreementError"),
        }
    }
}
