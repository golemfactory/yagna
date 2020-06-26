use chrono::{Duration, Utc};

use ya_client::model::market::{Demand as ClientDemand, Offer as ClientOffer};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::db::dao::*;
use crate::db::models::{Demand, Offer, SubscriptionId};
use crate::matcher::error::{DemandError, OfferError, UnsubscribeError};
use crate::matcher::MatcherError;
use crate::protocol::{Propagate, Reason};

#[derive(Clone)]
pub struct SubscriptionStore {
    db: DbExecutor,
}

impl SubscriptionStore {
    pub fn new(db: DbExecutor) -> Self {
        Self { db }
    }

    pub async fn create_offer(
        &self,
        id: &Identity,
        offer: &ClientOffer,
    ) -> Result<Offer, OfferError> {
        let creation_ts = Utc::now().naive_utc();
        // TODO: provider agent should set expiration.
        let expiration_ts = creation_ts + Duration::hours(24);
        let offer = Offer::from_new(offer, &id, creation_ts, expiration_ts);
        self.db.as_dao::<OfferDao>().insert(offer.clone()).await?;
        Ok(offer)
    }

    pub async fn mark_offer_unsubscribed(&self, id: &SubscriptionId) -> Result<(), OfferError> {
        let now = Utc::now().naive_utc();
        self.db
            .as_dao::<OfferDao>()
            .mark_unsubscribed(id, now)
            .await
            .map_err(|e| OfferError::UnsubscribeError(e.into(), id.clone()))
            .and_then(|state| match state {
                OfferState::Active(_) => Ok(()),
                OfferState::NotFound => Err(OfferError::NotExists(id.clone())),
                OfferState::Unsubscribed(_) => Err(OfferError::UnsubscribeError(
                    UnsubscribeError::AlreadyUnsubscribed,
                    id.clone(),
                )),
                OfferState::Expired(_) => Err(OfferError::UnsubscribeError(
                    UnsubscribeError::OfferExpired,
                    id.clone(),
                )),
            })
    }

    pub async fn get_offer(&self, id: &SubscriptionId) -> Result<Option<Offer>, OfferError> {
        let now = Utc::now().naive_utc();
        match self.db.as_dao::<OfferDao>().select(id, now).await? {
            OfferState::Active(offer) => Ok(Some(offer)),
            _ => Ok(None),
        }
    }

    pub async fn checked_remove_offer(
        &self,
        id: &SubscriptionId,
    ) -> Result<Propagate, MatcherError> {
        let now = Utc::now().naive_utc();
        match self
            .db
            .as_dao::<OfferDao>()
            .mark_unsubscribed(id, now)
            .await?
        {
            OfferState::Expired(_) => Ok(Propagate::No(Reason::Expired)),
            OfferState::Unsubscribed(_) => Ok(Propagate::No(Reason::Unsubscribed)),
            OfferState::NotFound => Ok(Propagate::No(Reason::NotFound)),
            OfferState::Active(_) => {
                // We store only our Offers to keep history. Offers from other nodes
                // should be removed.
                // We are sure that we don't remove our Offer here, because we would got
                // `AlreadyUnsubscribed` error from `checked_unsubscribe_offer`,
                // as it was already invoked before broadcast in `unsubscribe_offer`.
                // TODO: Maybe we should add check here, to be sure, that we don't remove own Offers.
                log::debug!("Removing unsubscribed Offer [{}].", id);
                let _ = self.remove_offer(id).await.map_err(|e| {
                    log::warn!("Failed to remove offer [{}] during unsubscribe: {}", id, e);
                });
                Ok(Propagate::Yes)
            }
        }
    }

    pub async fn checked_store_offer(&self, mut offer: Offer) -> Result<Propagate, MatcherError> {
        // Will reject Offer, if hash was computed incorrectly. In most cases
        // it could mean, that it could be some kind of attack.
        offer.validate()?;

        // We shouldn't propagate Offer, if we already have it in our database.
        // Note that when, we broadcast our Offer, it will reach us too, so it concerns
        // not only Offers from other nodes.
        //
        // Note: Infinite broadcasting is possible here, if we would just use get_offer function,
        // because it filters expired and unsubscribed Offers. Note what happens in such case:
        // We think that Offer doesn't exist, so we insert it to database every time it reaches us,
        // because get_offer will never return it. So we will never meet stop condition of broadcast!!
        // So be careful.
        let now = Utc::now().naive_utc();
        match self.db.as_dao::<OfferDao>().select(&offer.id, now).await? {
            OfferState::Active(_) => Ok(Propagate::No(Reason::AlreadyExists)),
            OfferState::Unsubscribed(_) => Ok(Propagate::No(Reason::Unsubscribed)),
            OfferState::Expired(_) => Ok(Propagate::No(Reason::Expired)),
            OfferState::NotFound => {
                // Insertions timestamp should always reference our local time
                // of adding it to database, so we must reset it here.
                offer.insertion_ts = None;
                self.db.as_dao::<OfferDao>().insert(offer).await?;
                Ok(Propagate::Yes)
            }
        }
    }

    pub async fn remove_offer(&self, id: &SubscriptionId) -> Result<(), OfferError> {
        match self
            .db
            .as_dao::<DemandDao>()
            .delete(&id)
            .await
            .map_err(|e| OfferError::RemoveError(e, id.clone()))?
        {
            true => Ok(()),
            false => Err(OfferError::NotExists(id.clone())),
        }
    }

    pub async fn create_demand(
        &self,
        id: &Identity,
        demand: &ClientDemand,
    ) -> Result<Demand, DemandError> {
        let creation_ts = Utc::now().naive_utc();
        // TODO: requestor agent should set expiration.
        let expiration_ts = creation_ts + Duration::hours(24);
        let demand = Demand::from_new(demand, &id, creation_ts, expiration_ts);
        self.db.as_dao::<DemandDao>().insert(&demand).await?;
        Ok(demand)
    }

    pub async fn get_demand(&self, id: &SubscriptionId) -> Result<Option<Demand>, DemandError> {
        Ok(self.db.as_dao::<DemandDao>().select(id).await?)
    }

    pub async fn remove_demand(&self, id: &SubscriptionId) -> Result<(), DemandError> {
        match self
            .db
            .as_dao::<DemandDao>()
            .delete(&id)
            .await
            .map_err(|e| DemandError::RemoveError(e, id.clone()))?
        {
            true => Ok(()),
            false => Err(DemandError::NotExists(id.clone())),
        }
    }
}
