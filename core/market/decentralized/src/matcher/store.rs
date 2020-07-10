use chrono::{Duration, NaiveDateTime, Utc};
use lazy_static::lazy_static;

use ya_client::model::market::{Demand as ClientDemand, Offer as ClientOffer};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::db::dao::*;
use crate::db::models::{Demand, Offer, SubscriptionId};
use crate::matcher::error::{
    DemandError, ModifyOfferError, QueryOfferError, QueryOffersError, SaveOfferError,
};

lazy_static! {
    // TODO: agents should set expiration.
    static ref DEFAULT_TTL: Duration = Duration::hours(24);
}

#[derive(Clone)]
pub struct SubscriptionStore {
    db: DbExecutor,
}

impl SubscriptionStore {
    pub fn new(db: DbExecutor) -> Self {
        Self { db }
    }

    /// returns newly created offer with insertion_ts
    pub async fn create_offer(
        &self,
        id: &Identity,
        offer: &ClientOffer,
    ) -> Result<Offer, SaveOfferError> {
        let creation_ts = Utc::now().naive_utc();
        // TODO: provider agent should set expiration.
        let expiration_ts = creation_ts + *DEFAULT_TTL;
        let offer = Offer::from_new(offer, &id, creation_ts, expiration_ts);
        self.insert_offer(offer).await
    }

    /// returns saved offer with insertion_ts
    pub async fn save_offer(&self, offer: Offer) -> Result<Offer, SaveOfferError> {
        offer.validate()?;
        self.insert_offer(offer).await
    }

    async fn insert_offer(&self, mut offer: Offer) -> Result<Offer, SaveOfferError> {
        // Insertions timestamp should always reference our local time
        // of adding it to database, so we must reset it here.
        offer.insertion_ts = None;
        let id = offer.id.clone();

        match self
            .db
            .as_dao::<OfferDao>()
            .insert(offer, Utc::now().naive_utc())
            .await
        {
            Ok((true, OfferState::Active(offer))) => Ok(offer),
            Ok((false, OfferState::Active(_))) => Err(SaveOfferError::Exists(id)),
            Ok((false, OfferState::Unsubscribed(_))) => Err(SaveOfferError::Unsubscribed(id)),
            Ok((_, OfferState::Expired(_))) => Err(SaveOfferError::Expired(id)),
            Ok((inserted, state)) => Err(SaveOfferError::UnexpectedError(format!(
                "Offer [{}] state: {:?} after insert: {}",
                id, state, inserted
            ))),
            Err(e) => Err(SaveOfferError::SaveError(e, id)),
        }
    }

    pub async fn get_offers(
        &self,
        id: Option<Identity>,
    ) -> Result<Vec<ClientOffer>, QueryOffersError> {
        Ok(self
            .db
            .as_dao::<OfferDao>()
            .get_offers(id.map(|ident| ident.identity), Utc::now().naive_utc())
            .await
            .map_err(|e| QueryOffersError(e))?
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

    pub async fn get_offers_before(
        &self,
        insertion_ts: NaiveDateTime,
    ) -> Result<Vec<Offer>, QueryOffersError> {
        Ok(self
            .db
            .as_dao::<OfferDao>()
            .get_offers_before(insertion_ts, Utc::now().naive_utc())
            .await
            .map_err(|e| QueryOffersError(e))?)
    }

    pub async fn get_offer(&self, id: &SubscriptionId) -> Result<Offer, QueryOfferError> {
        let now = Utc::now().naive_utc();
        match self.db.as_dao::<OfferDao>().select(id, now).await {
            Err(e) => Err(QueryOfferError::Get(e, id.clone())),
            Ok(OfferState::Active(offer)) => Ok(offer),
            Ok(OfferState::Unsubscribed(_)) => {
                Err(QueryOfferError::AlreadyUnsubscribed(id.clone()))
            }
            Ok(OfferState::Expired(_)) => Err(QueryOfferError::Expired(id.clone())),
            Ok(OfferState::NotFound) => Err(QueryOfferError::NotFound(id.clone())),
        }
    }

    pub async fn mark_offer_unsubscribed(
        &self,
        id: &SubscriptionId,
    ) -> Result<(), ModifyOfferError> {
        self.db
            .as_dao::<OfferDao>()
            .mark_unsubscribed(id, Utc::now().naive_utc())
            .await
            .map_err(|e| ModifyOfferError::UnsubscribeError(e.into(), id.clone()))
            .and_then(|state| match state {
                OfferState::Active(_) => Ok(()),
                OfferState::NotFound => Err(ModifyOfferError::NotFound(id.clone())),
                OfferState::Unsubscribed(_) => {
                    Err(ModifyOfferError::AlreadyUnsubscribed(id.clone()))
                }
                OfferState::Expired(_) => Err(ModifyOfferError::Expired(id.clone())),
            })
    }

    /// We store only our Offers to keep history. Offers from other nodes
    /// should be removed.
    /// This is meant to be called upon receiving unsubscribe broadcast. To work correctly
    /// it assumes `mark_offer_unsubscribed` was invoked before broadcast in `unsubscribe_offer`.
    pub async fn unsubscribe_offer(&self, id: &SubscriptionId) -> Result<(), ModifyOfferError> {
        // If `mark_offer_unsubscribed` was called before we won't remove our Offer here,
        // because `AlreadyUnsubscribed` error will pop-up.
        self.mark_offer_unsubscribed(id).await?;
        // TODO: Maybe we should add check here, to be sure, that we don't remove own Offers.
        log::debug!("Removing unsubscribed Offer [{}].", id);
        match self.db.as_dao::<OfferDao>().delete(&id).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(ModifyOfferError::UnexpectedError(format!(
                "Offer [{}] marked as unsubscribed, but not removed",
                id
            ))),
            Err(e) => Err(ModifyOfferError::RemoveError(e, id.clone()).into()),
        }
    }

    pub async fn create_demand(
        &self,
        id: &Identity,
        demand: &ClientDemand,
    ) -> Result<Demand, DemandError> {
        let creation_ts = Utc::now().naive_utc();
        // TODO: requestor agent should set expiration.
        let expiration_ts = creation_ts + *DEFAULT_TTL;
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

    pub async fn get_demands_before(
        &self,
        insertion_ts: NaiveDateTime,
    ) -> Result<Vec<Demand>, DemandError> {
        Ok(self
            .db
            .as_dao::<DemandDao>()
            .get_demands_before(insertion_ts, Utc::now().naive_utc())
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
