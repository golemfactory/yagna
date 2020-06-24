use ya_client::model::ErrorMessage;
use ya_persistence::executor::Error as DbError;

use crate::db::dao::UnsubscribeError;
use crate::db::models::{SubscriptionId, SubscriptionValidationError};
use crate::protocol::DiscoveryInitError;

#[derive(thiserror::Error, Debug)]
pub enum DemandError {
    #[error("Failed to save Demand. Error: {0}.")]
    SaveDemandFailure(#[from] DbError),
    #[error("Failed to remove Demand [{1}]. Error: {0}.")]
    RemoveDemandFailure(DbError, SubscriptionId),
    #[error("Demand [{0}] doesn't exist.")]
    DemandNotExists(SubscriptionId),
}

#[derive(thiserror::Error, Debug)]
pub enum OfferError {
    #[error("Failed to save Offer. Error: {0}.")]
    SaveOfferFailure(#[from] DbError),
    #[error("Failed to unsubscribe Offer [{1}]. Error: {0}.")]
    UnsubscribeOfferFailure(UnsubscribeError, SubscriptionId),
    #[error("Offer [{0}] doesn't exist.")]
    OfferNotExists(SubscriptionId),
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
