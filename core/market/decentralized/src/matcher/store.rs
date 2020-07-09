use chrono::{Duration, Utc};

use ya_client::model::market::{Demand as ClientDemand, Offer as ClientOffer};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::db::dao::*;
use crate::db::models::{Demand, Offer, SubscriptionId};
use crate::matcher::error::{DemandError, OfferError};

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
        self.save_offer(offer.clone()).await?;
        Ok(offer)
    }

    async fn save_offer(&self, mut offer: Offer) -> Result<(), OfferError> {
        let id = offer.id.clone();
        // Insertions timestamp should always reference our local time
        // of adding it to database, so we must reset it here.
        offer.insertion_ts = None;
        self.db
            .as_dao::<OfferDao>()
            .insert(offer)
            .await
            .map_err(|e| OfferError::SaveError(e, id))
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
                OfferState::NotFound => Err(OfferError::NotFound(id.clone())),
                OfferState::Unsubscribed(_) => Err(OfferError::AlreadyUnsubscribed(id.clone())),
                OfferState::Expired(_) => Err(OfferError::Expired(id.clone())),
            })
    }

    pub async fn get_offers(&self, id: Option<Identity>) -> Result<Vec<ClientOffer>, OfferError> {
        Ok(self
            .db
            .as_dao::<OfferDao>()
            .get_offers(id.map(|ident| ident.identity))
            .await
            .map_err(|e| OfferError::GetMany(e))?
            .into_iter()
            .filter_map(|o| match o.into_client_offer() {
                Err(e) => {
                    log::error!("Skipping Offer because of: {}", e);
                    None
                }
                Ok(o) => Some(o),
            })
            .collect())
    }

    pub async fn get_offers_before(&self, demand: &Demand) -> Result<Vec<Offer>, OfferError> {
        let now = Utc::now().naive_utc();
        Ok(self
            .db
            .as_dao::<OfferDao>()
            .get_offers_before(demand.insertion_ts.unwrap(), now)
            .await
            .map_err(|e| OfferError::GetMany(e))?)
    }

    pub async fn get_offer(&self, id: &SubscriptionId) -> Result<Offer, OfferError> {
        let now = Utc::now().naive_utc();
        match self.db.as_dao::<OfferDao>().select(id, now).await {
            Err(e) => Err(OfferError::GetError(e, id.clone())),
            Ok(OfferState::Active(offer)) => Ok(offer),
            Ok(OfferState::Unsubscribed(_)) => Err(OfferError::AlreadyUnsubscribed(id.clone())),
            Ok(OfferState::Expired(_)) => Err(OfferError::Expired(id.clone())),
            Ok(OfferState::NotFound) => Err(OfferError::NotFound(id.clone())),
        }
    }

    /// We store only our Offers to keep history. Offers from other nodes
    /// should be removed.
    /// This is meant to be called upon receiving unsubscribe broadcast. To work correctly
    /// it assumes `mark_offer_unsubscribed` was invoked before broadcast in `unsubscribe_offer`.
    pub async fn remove_offer(&self, id: &SubscriptionId) -> Result<(), OfferError> {
        // If `mark_offer_unsubscribed` was called before we won't remove our Offer here,
        // because `AlreadyUnsubscribed` error will pop-up.
        self.mark_offer_unsubscribed(id).await?;
        // TODO: Maybe we should add check here, to be sure, that we don't remove own Offers.
        log::debug!("Removing unsubscribed Offer [{}].", id);
        match self.db.as_dao::<OfferDao>().delete(&id).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(OfferError::UnexpectedError(format!(
                "Offer [{}] marked as unsubscribed, but not removed",
                id
            ))),
            Err(e) => Err(OfferError::RemoveError(e, id.clone()).into()),
        }
    }

    pub async fn store_offer(&self, offer: Offer) -> Result<bool, OfferError> {
        // Will reject Offer, if hash was computed incorrectly. In most cases
        // it could mean, that it could be some kind of attack.
        offer.validate()?;

        let id = offer.id.clone();
        match self.get_offer(&id).await {
            Ok(_offer) => Ok(false),
            Err(OfferError::NotFound(_)) => {
                self.save_offer(offer).await?;
                Ok(true)
            }
            Err(e) => Err(e),
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
        self.db
            .as_dao::<DemandDao>()
            .insert(&demand)
            .await
            .map_err(|e| DemandError::SaveError(e))?;
        Ok(demand)
    }

    pub async fn get_demand(&self, id: &SubscriptionId) -> Result<Demand, DemandError> {
        match self.db.as_dao::<DemandDao>().select(id).await {
            Err(e) => Err(DemandError::GetError(e, id.clone())),
            Ok(Some(demand)) => Ok(demand),
            Ok(None) => Err(DemandError::NotFound(id.clone())),
        }
    }

    pub async fn get_demands_before(&self, offer: &Offer) -> Result<Vec<Demand>, DemandError> {
        let now = Utc::now().naive_utc();
        Ok(self
            .db
            .as_dao::<DemandDao>()
            .get_demands_before(offer.insertion_ts.unwrap(), now)
            .await
            .map_err(|e| DemandError::GetMany(e))?)
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
            false => Err(DemandError::NotFound(id.clone())),
        }
    }
}
