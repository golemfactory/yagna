use crate::protocol::negotiation::error::{
    CounterProposalError as ApiProposalError, NegotiationApiInitError,
};
use thiserror::Error;

use super::common::GetProposalError;
use crate::db::model::{
    AgreementId, ProposalId, ProposalIdParseError, SubscriptionId, SubscriptionParseError,
};
use crate::db::{dao::TakeEventsError, DbError};

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
    #[error("Invalid proposal id. {0}")]
    Protocol(#[from] crate::protocol::negotiation::error::AgreementError),
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
    #[error("Subscription [{0}] was already unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Subscription [{0}] expired.")]
    SubscriptionExpired(SubscriptionId),
    #[error("Proposal [{0}] not found for subscription [{1}].")]
    ProposalNotFound(ProposalId, SubscriptionId),
    #[error("Failed to get Proposal [{0}] for subscription [{1}]. Error: [{2}]")]
    FailedGetProposal(ProposalId, SubscriptionId, DbError),
    #[error("Failed to save counter Proposal for Proposal [{0}]. Error: {1}")]
    FailedSaveProposal(ProposalId, DbError),
    #[error("Failed to send counter Proposal for Proposal [{0}]. Error: {1}")]
    FailedSendProposal(ProposalId, ApiProposalError),
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
