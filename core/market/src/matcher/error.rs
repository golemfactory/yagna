use crate::db::model::{SubscriptionId, SubscriptionValidationError};
use crate::db::DbError;
use crate::identity::IdentityError;
use crate::protocol::discovery::error::DiscoveryInitError;

#[derive(thiserror::Error, Debug)]
pub enum DemandError {
    #[error("Failed to get Demands. Error: {0}.")]
    GetMany(DbError),
    #[error("Failed to get Demand [{1}]. Error: {0}.")]
    GetSingle(DbError, SubscriptionId),
    #[error("Failed to save Demand. Error: {0}.")]
    Save(DbError),
    #[error("Failed to remove Demand [{1}]. Error: {0}.")]
    Remove(DbError, SubscriptionId),
    #[error("Demand [{0}] not found.")]
    NotFound(SubscriptionId),
    #[error(transparent)]
    JsonObjectExpected(#[from] serde_json::error::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum QueryOffersError {
    #[error("Failed to get Offers. Error: {0}.")]
    DbError(#[from] DbError),
    #[error("Failed to list Offers based on identity. Error: {0}.")]
    IdentityError(#[from] IdentityError),
}

#[derive(thiserror::Error, Debug)]
pub enum QueryDemandsError {
    #[error("Failed to get Demands. Error: {0}.")]
    DbError(#[from] DbError),
}

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
    Save(DbError, SubscriptionId),
    #[error("Failed to save already existing Offer [{0}].")]
    Exists(SubscriptionId),
    #[error("Offer [{0}] already unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Offer [{0}] expired.")]
    Expired(SubscriptionId),
    #[error(transparent)]
    SubscriptionValidation(#[from] SubscriptionValidationError),
    #[error(transparent)]
    JsonObjectExpected(#[from] serde_json::error::Error),
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
    AlreadyUnsubscribed(SubscriptionId),
    #[error("Can't unsubscribe expired Offer [{0}].")]
    Expired(SubscriptionId),
    #[error("Failed to unsubscribe Offer [{1}]. Error: {0}")]
    Unsubscribe(DbError, SubscriptionId),
    #[error("Failed to remove Offer [{1}]. Error: {0}.")]
    Remove(DbError, SubscriptionId),
    #[error("Offer [{0}] marked as unsubscribed, but not removed")]
    UnsubscribedNotRemoved(SubscriptionId),
}

impl From<QueryOfferError> for ModifyOfferError {
    fn from(e: QueryOfferError) -> Self {
        match e {
            QueryOfferError::NotFound(id) | QueryOfferError::Get(_, id) => {
                ModifyOfferError::NotFound(id)
            }
            QueryOfferError::Unsubscribed(id) => ModifyOfferError::AlreadyUnsubscribed(id),
            QueryOfferError::Expired(id) => ModifyOfferError::Expired(id),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum MatcherError {
    #[error(transparent)]
    Demand(#[from] DemandError),
    #[error(transparent)]
    QueryOffers(#[from] QueryOffersError),
    #[error(transparent)]
    QueryOffer(#[from] QueryOfferError),
    #[error(transparent)]
    SaveOffer(#[from] SaveOfferError),
    #[error(transparent)]
    ModifyOffer(#[from] ModifyOfferError),
}

#[derive(thiserror::Error, Debug)]
pub enum MatcherInitError {
    #[error("Failed to initialize Discovery interface. Error: {0}.")]
    DiscoveryInitError(#[from] DiscoveryInitError),
    #[error("Failed to initialize expiration tracker. Error: {0}.")]
    ExpirationTrackerError(String),
}

#[derive(thiserror::Error, Debug)]
pub enum ResolverError {
    #[error(transparent)]
    QueryOffer(#[from] QueryOfferError),
    #[error(transparent)]
    QueryOffers(#[from] QueryOffersError),
    #[error(transparent)]
    Demand(#[from] DemandError),
}

impl From<ResolverError> for MatcherError {
    fn from(e: ResolverError) -> Self {
        match e {
            ResolverError::QueryOffer(e) => MatcherError::QueryOffer(e),
            ResolverError::QueryOffers(e) => MatcherError::QueryOffers(e),
            ResolverError::Demand(e) => MatcherError::Demand(e),
        }
    }
}
