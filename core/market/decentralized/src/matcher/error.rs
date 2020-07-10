use ya_client::model::ErrorMessage;
use ya_persistence::executor::Error as DbError;

use crate::db::models::{SubscriptionId, SubscriptionValidationError};
use crate::protocol::DiscoveryInitError;

#[derive(thiserror::Error, Debug)]
pub enum DemandError {
    #[error("Failed to get Offers. Error: {0}.")]
    GetMany(DbError),
    #[error("Failed to get Demand [{1}]. Error: {0}.")]
    GetError(DbError, SubscriptionId),
    #[error("Failed to save Demand. Error: {0}.")]
    SaveError(DbError),
    #[error("Failed to remove Demand [{1}]. Error: {0}.")]
    RemoveError(DbError, SubscriptionId),
    #[error("Demand [{0}] not found.")]
    NotFound(SubscriptionId),
}

#[derive(thiserror::Error, Debug)]
#[error("Failed to get Offers. Error: {0}.")]
pub struct QueryOffersError(pub DbError);

#[derive(thiserror::Error, Debug)]
pub enum QueryOfferError {
    #[error("Offer [{0}] not found.")]
    NotFound(SubscriptionId),
    #[error("Failed to get Offer [{1}]. Error: {0}.")]
    Get(DbError, SubscriptionId),
    #[error("Offer [{0}] already unsubscribed.")]
    AlreadyUnsubscribed(SubscriptionId),
    #[error("Can't unsubscribe expired Offer [{0}].")]
    Expired(SubscriptionId),
    #[error("Unexpected Offer error: {0}.")]
    UnexpectedError(String),
}

#[derive(thiserror::Error, Debug)]
pub enum SaveOfferError {
    #[error("Failed to save Offer [{1}]. Error: {0}.")]
    SaveError(DbError, SubscriptionId),
    #[error("Failed to save already existing Offer [{0}].")]
    Exists(SubscriptionId),
    #[error("Offer [{0}] already unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Offer [{0}] expired.")]
    Expired(SubscriptionId),
    #[error(transparent)]
    SubscriptionValidation(#[from] SubscriptionValidationError),
    #[error("Unexpected Offer error: {0}.")]
    UnexpectedError(String),
}

#[derive(thiserror::Error, Debug)]
pub enum ModifyOfferError {
    #[error("Offer [{0}] not found.")]
    NotFound(SubscriptionId),
    #[error("Offer [{0}] already unsubscribed.")]
    AlreadyUnsubscribed(SubscriptionId),
    #[error("Failed to unsubscribe Offer [{1}]. Error: {0}")]
    UnsubscribeError(DbError, SubscriptionId),
    #[error("Can't unsubscribe expired Offer [{0}].")]
    Expired(SubscriptionId),
    #[error("Failed to remove Offer [{1}]. Error: {0}.")]
    RemoveError(DbError, SubscriptionId),
    #[error("Unexpected Offer error: {0}.")]
    UnexpectedError(String),
}

#[derive(thiserror::Error, Debug)]
pub enum MatcherError {
    #[error(transparent)]
    DemandError(#[from] DemandError),
    #[error(transparent)]
    QueryOfferError(#[from] QueryOfferError),
    #[error(transparent)]
    ModifyOfferError(#[from] ModifyOfferError),
    #[error(transparent)]
    SaveOfferError(#[from] SaveOfferError),
    #[error(transparent)]
    ResolverError(#[from] ResolverError),
    #[error("Unexpected Matcher error: {0}.")]
    UnexpectedError(String),
}

#[derive(thiserror::Error, Debug)]
pub enum MatcherInitError {
    #[error("Failed to initialize Discovery interface. Error: {0}.")]
    DiscoveryError(#[from] DiscoveryInitError),
    #[error("Failed to initialize database. Error: {0}.")]
    DatabaseError(#[from] DbError),
}

#[derive(thiserror::Error, Debug)]
pub enum ResolverError {
    #[error(transparent)]
    QueryOfferError(#[from] QueryOfferError),
    #[error(transparent)]
    QueryOffersError(#[from] QueryOffersError),
    #[error(transparent)]
    DemandError(#[from] DemandError),
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
