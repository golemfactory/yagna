use crate::protocol::negotiation::errors::{
    CounterProposalError as ApiProposalError, NegotiationApiInitError,
};
use thiserror::Error;

use crate::db::dao::TakeEventsError;
use crate::db::models::{SubscriptionId, SubscriptionParseError};

use ya_persistence::executor::Error as DbError;

#[derive(Error, Debug)]
pub enum NegotiationError {}

#[derive(Error, Debug)]
pub enum NegotiationInitError {
    #[error("Failed to initialize Negotiation interface. Error: {0}.")]
    ApiInitError(#[from] NegotiationApiInitError),
}

#[derive(Error, Debug)]
pub enum QueryEventsError {
    #[error("Subscription [{0}] was already unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Subscription [{0}] expired.")]
    SubscriptionExpired(SubscriptionId),
    #[error("Invalid subscription id. {0}")]
    InvalidSubscriptionId(#[from] SubscriptionParseError),
    #[error("Failed to get events from database. Error: {0}.")]
    FailedGetEvents(TakeEventsError),
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
    ProposalNotFound(String, SubscriptionId),
    #[error("Failed to get Proposal [{0}]. Error: [{1}]")]
    FailedGetProposal(String, String),
    #[error("Failed to save counter Proposal for Proposal [{0}]. Error: {1}")]
    FailedSaveProposal(String, DbError),
    #[error("Failed to send counter Proposal for Proposal [{0}]. Error: {1}")]
    FailedSendProposal(String, ApiProposalError),
}

impl From<TakeEventsError> for QueryEventsError {
    fn from(e: TakeEventsError) -> Self {
        match e {
            TakeEventsError::SubscriptionExpired(id) => QueryEventsError::SubscriptionExpired(id),
            TakeEventsError::SubscriptionNotFound(id) => QueryEventsError::Unsubscribed(id),
            _ => QueryEventsError::FailedGetEvents(e),
        }
    }
}
