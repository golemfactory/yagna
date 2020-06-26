use ya_client::model::ErrorMessage;
use ya_persistence::executor::Error as DbError;

use crate::db::models::{SubscriptionId, SubscriptionValidationError};
use crate::protocol::DiscoveryInitError;

#[derive(thiserror::Error, Debug)]
pub enum DemandError {
    #[error("Failed to save Demand. Error: {0}.")]
    SaveError(#[from] DbError),
    #[error("Failed to remove Demand [{1}]. Error: {0}.")]
    RemoveError(DbError, SubscriptionId),
    #[error("Demand [{0}] doesn't exist.")]
    NotExists(SubscriptionId),
}

#[derive(thiserror::Error, Debug)]
pub enum OfferError {
    #[error("Failed to save Offer. Error: {0}.")]
    SaveError(#[from] DbError),
    #[error("Failed to unsubscribe Offer [{1}]. Error: {0}.")]
    UnsubscribeError(UnsubscribeError, SubscriptionId),
    #[error("Failed to remove Demand [{1}]. Error: {0}.")]
    RemoveError(DbError, SubscriptionId),
    #[error("Offer [{0}] doesn't exist.")]
    NotExists(SubscriptionId),
}

#[derive(thiserror::Error, Debug)]
pub enum UnsubscribeError {
    #[error("Can't unsubscribe expired offer")]
    OfferExpired,
    #[error("Offer already unsubscribed")]
    AlreadyUnsubscribed,
    #[error("Can't unsubscribe offer. Database error: {0}")]
    DatabaseError(DbError),
}

impl<E: Into<DbError>> From<E> for UnsubscribeError {
    fn from(e: E) -> Self {
        UnsubscribeError::DatabaseError(e.into())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum MatcherError {
    #[error(transparent)]
    DemandError(#[from] DemandError),
    #[error(transparent)]
    OfferError(#[from] OfferError),
    #[error(transparent)]
    SubscriptionValidation(#[from] SubscriptionValidationError),
    #[error("Unexpected Internal error: {0}.")]
    UnexpectedError(String),
}

#[derive(thiserror::Error, Debug)]
pub enum MatcherInitError {
    #[error("Failed to initialize Discovery interface. Error: {0}.")]
    DiscoveryError(#[from] DiscoveryInitError),
    #[error("Failed to initialize database. Error: {0}.")]
    DatabaseError(#[from] DbError),
}

impl From<ErrorMessage> for MatcherError {
    fn from(e: ErrorMessage) -> Self {
        MatcherError::UnexpectedError(e.to_string())
    }
}

impl From<DbError> for MatcherError {
    fn from(e: DbError) -> Self {
        MatcherError::UnexpectedError(e.to_string())
    }
}
