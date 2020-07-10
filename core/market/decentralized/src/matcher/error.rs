use ya_persistence::executor::Error as DbError;

use crate::db::models::{SubscriptionId, SubscriptionValidationError};
use crate::protocol::DiscoveryInitError;

#[derive(thiserror::Error, Debug)]
pub enum DemandError {
    #[error("Failed to get Demands. Error: {0}.")]
    GetMany(DbError),
    #[error("Failed to get Demand [{1}]. Error: {0}.")]
    GetSingle(DbError, SubscriptionId),
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
    #[error("Offer [{0}] unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Offer [{0}] expired.")]
    Expired(SubscriptionId),
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
    #[error("Wrong Offer [{id}] state {state:?} after inserted: {inserted}.")]
    WrongState {
        state: String,
        inserted: bool,
        id: SubscriptionId,
    },
}

#[derive(thiserror::Error, Debug)]
pub enum ModifyOfferError {
    #[error("Offer [{0}] not found.")]
    NotFound(SubscriptionId),
    #[error("Offer [{0}] already unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Can't unsubscribe expired Offer [{0}].")]
    Expired(SubscriptionId),
    #[error("Failed to unsubscribe Offer [{1}]. Error: {0}")]
    UnsubscribeError(DbError, SubscriptionId),
    #[error("Failed to remove Offer [{1}]. Error: {0}.")]
    RemoveError(DbError, SubscriptionId),
    #[error("Offer [{0}] marked as unsubscribed, but not removed")]
    UnsubscribedNotRemoved(SubscriptionId),
}

#[derive(thiserror::Error, Debug)]
pub enum MatcherError {
    #[error(transparent)]
    DemandError(#[from] DemandError),
    #[error(transparent)]
    QueryOffersError(#[from] QueryOffersError),
    #[error(transparent)]
    QueryOfferError(#[from] QueryOfferError),
    #[error(transparent)]
    SaveOfferError(#[from] SaveOfferError),
    #[error(transparent)]
    ModifyOfferError(#[from] ModifyOfferError),
}

#[derive(thiserror::Error, Debug)]
pub enum MatcherInitError {
    #[error("Failed to initialize Discovery interface. Error: {0}.")]
    DiscoveryInitError(#[from] DiscoveryInitError),
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

impl From<ResolverError> for MatcherError {
    fn from(e: ResolverError) -> Self {
        match e {
            ResolverError::QueryOfferError(e) => MatcherError::QueryOfferError(e),
            ResolverError::QueryOffersError(e) => MatcherError::QueryOffersError(e),
            ResolverError::DemandError(e) => MatcherError::DemandError(e),
        }
    }
}
