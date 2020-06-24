use ya_persistence::executor::DbExecutor;

use crate::db::dao::*;
use crate::db::models::Demand;
use crate::db::models::Offer;
use crate::db::models::SubscriptionId;

use crate::matcher::error::{DemandError, OfferError};

#[derive(Clone)]
pub struct SubscriptionStore {
    db: DbExecutor,
}

impl SubscriptionStore {
    pub fn new(db: DbExecutor) -> Self {
        Self { db }
    }

    pub async fn create_offer(&self, offer: &Offer) -> Result<(), OfferError> {
        Ok(self.db.as_dao::<OfferDao>().create_offer(offer).await?)
    }

    pub async fn mark_offer_as_unsubscribed(&self, id: &SubscriptionId) -> Result<(), OfferError> {
        self.db
            .as_dao::<OfferDao>()
            .mark_offer_as_unsubscribed(id)
            .await
            .map_err(|e| OfferError::UnsubscribeOfferFailure(e, id.clone()))
    }

    pub async fn get_offer(&self, id: &SubscriptionId) -> Result<Option<Offer>, OfferError> {
        Ok(self.db.as_dao::<OfferDao>().get_offer(id).await?)
    }

    pub async fn get_offer_state(&self, id: &SubscriptionId) -> Result<OfferState, OfferError> {
        Ok(self.db.as_dao::<OfferDao>().get_offer_state(id).await?)
    }

    pub async fn create_demand(&self, demand: &Demand) -> Result<(), DemandError> {
        Ok(self.db.as_dao::<DemandDao>().create_demand(demand).await?)
    }

    pub async fn get_demand(&self, id: &SubscriptionId) -> Result<Option<Demand>, DemandError> {
        Ok(self.db.as_dao::<DemandDao>().get_demand(id).await?)
    }

    pub async fn remove_demand(&self, id: &SubscriptionId) -> Result<(), DemandError> {
        match self
            .db
            .as_dao::<DemandDao>()
            .remove_demand(&id)
            .await
            .map_err(|e| DemandError::RemoveDemandFailure(e, id.clone()))?
        {
            true => Ok(()),
            false => Err(DemandError::DemandNotExists(id.clone())),
        }
    }
}
