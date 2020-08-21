use chrono::{Duration, NaiveDateTime, Utc};
use lazy_static::lazy_static;

use ya_client::model::market::{Demand as ClientDemand, Offer as ClientOffer};
use ya_client::model::NodeId;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::db::dao::*;
use crate::db::model::{Demand, Offer, SubscriptionId};
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
            Ok((inserted, state)) => Err(SaveOfferError::WrongState {
                state: state.to_string(),
                inserted,
                id,
            }),
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
            Ok(OfferState::Unsubscribed(_)) => Err(QueryOfferError::Unsubscribed(id.clone())),
            Ok(OfferState::Expired(_)) => Err(QueryOfferError::Expired(id.clone())),
            Ok(OfferState::NotFound) => Err(QueryOfferError::NotFound(id.clone())),
        }
    }

    async fn mark_offer_unsubscribed(&self, id: &SubscriptionId) -> Result<(), ModifyOfferError> {
        self.db
            .as_dao::<OfferDao>()
            .mark_unsubscribed(id, Utc::now().naive_utc())
            .await
            .map_err(|e| ModifyOfferError::UnsubscribeError(e.into(), id.clone()))
            .and_then(|state| match state {
                OfferState::Active(_) => Ok(()),
                OfferState::NotFound => Err(ModifyOfferError::NotFound(id.clone())),
                OfferState::Unsubscribed(_) => Err(ModifyOfferError::Unsubscribed(id.clone())),
                OfferState::Expired(_) => Err(ModifyOfferError::Expired(id.clone())),
            })
    }

    /// Local Offers are kept after unsubscribe. Offers from other nodes are removed.
    pub async fn unsubscribe_offer(
        &self,
        offer_id: &SubscriptionId,
        local_caller: bool,
        caller_id: Option<NodeId>,
    ) -> Result<(), ModifyOfferError> {
        if let Ok(offer) = self.get_offer(offer_id).await {
            if caller_id != Some(offer.node_id) {
                // TODO: unauthorized?
                return Err(ModifyOfferError::NotFound(offer_id.clone()));
            }
        }

        // If this fn was called before, we won't remove our Offer below,
        // because `Unsubscribed` error will pop-up here.
        self.mark_offer_unsubscribed(offer_id).await?;

        if local_caller {
            // Local Offers we mark as unsubscribed only
            return Ok(());
        }

        log::debug!("Removing not owned unsubscribed Offer [{}].", offer_id);
        match self.db.as_dao::<OfferDao>().delete(&offer_id).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(ModifyOfferError::UnsubscribedNotRemoved(offer_id.clone())),
            Err(e) => Err(ModifyOfferError::RemoveError(e, offer_id.clone())),
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
            Err(e) => Err(DemandError::GetSingle(e, id.clone())),
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

    pub async fn remove_demand(
        &self,
        demand_id: &SubscriptionId,
        id: &Identity,
    ) -> Result<(), DemandError> {
        let demand = self.get_demand(demand_id).await?;
        if id.identity != demand.node_id {
            return Err(DemandError::NotFound(demand_id.clone()));
        }

        match self
            .db
            .as_dao::<DemandDao>()
            .delete(&demand_id)
            .await
            .map_err(|e| DemandError::RemoveError(e, demand_id.clone()))?
        {
            true => Ok(()),
            false => Err(DemandError::NotFound(demand_id.clone())),
        }
    }
}
