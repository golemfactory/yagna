use chrono::{NaiveDateTime, Utc};
use std::collections::HashSet;
use std::sync::Arc;

use ya_client::model::market::{Demand as ClientDemand, NewDemand, NewOffer, Offer as ClientOffer};
use ya_client::model::NodeId;
use ya_service_api_web::middleware::Identity;

use crate::config::Config;
use crate::db::dao::*;
use crate::db::model::{Demand, Offer, SubscriptionId};
use crate::db::DbMixedExecutor;
use crate::matcher::error::{
    DemandError, ModifyOfferError, QueryDemandsError, QueryOfferError, QueryOffersError,
    SaveOfferError,
};

#[derive(Clone)]
pub struct SubscriptionStore {
    pub(crate) db: DbMixedExecutor,
    config: Arc<Config>,
}

impl SubscriptionStore {
    pub fn new(db: DbMixedExecutor, config: Arc<Config>) -> Self {
        SubscriptionStore { db, config }
    }

    /// returns newly created offer with insertion_ts
    pub async fn create_offer(
        &self,
        id: &Identity,
        offer: &NewOffer,
    ) -> Result<Offer, SaveOfferError> {
        let creation_ts = Utc::now().naive_utc();
        // TODO: provider agent should set expiration.
        let expiration_ts = creation_ts + self.config.subscription.default_ttl;
        let offer = Offer::from_new(offer, &id, creation_ts, expiration_ts)?;
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
            .put(offer, Utc::now().naive_utc())
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
            Err(e) => Err(SaveOfferError::Save(e, id)),
        }
    }

    pub async fn get_active_offer_ids(
        &self,
        node_ids: Option<Vec<NodeId>>,
    ) -> Result<Vec<SubscriptionId>, QueryOffersError> {
        Ok(self
            .db
            .as_dao::<OfferDao>()
            .get_offer_ids(node_ids, Utc::now().naive_utc())
            .await
            .map_err(QueryOffersError::from)?)
    }

    pub async fn get_unsubscribed_offer_ids(
        &self,
        node_ids: Option<Vec<NodeId>>,
    ) -> Result<Vec<SubscriptionId>, QueryOffersError> {
        Ok(self
            .db
            .as_dao::<OfferDao>()
            .get_unsubscribed_ids(node_ids, Utc::now().naive_utc())
            .await
            .map_err(QueryOffersError::from)?)
    }

    pub async fn get_client_offers(
        &self,
        node_id: Option<NodeId>,
    ) -> Result<Vec<ClientOffer>, QueryOffersError> {
        Ok(self
            .db
            .as_dao::<OfferDao>()
            .get_offers(
                None,
                node_id.map(|id| vec![id]),
                None,
                Utc::now().naive_utc(),
            )
            .await
            .map_err(QueryOffersError::from)?
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

    pub async fn get_offers(
        &self,
        ids: Vec<SubscriptionId>,
    ) -> Result<Vec<Offer>, QueryOffersError> {
        Ok(self
            .db
            .as_dao::<OfferDao>()
            .get_offers(Some(ids), None, None, Utc::now().naive_utc())
            .await
            .map_err(QueryOffersError::from)?)
    }

    pub async fn get_offers_before(
        &self,
        inserted_before_ts: NaiveDateTime,
    ) -> Result<Vec<Offer>, QueryOffersError> {
        Ok(self
            .db
            .as_dao::<OfferDao>()
            .get_offers(None, None, Some(inserted_before_ts), Utc::now().naive_utc())
            .await
            .map_err(QueryOffersError::from)?)
    }

    /// Returns Offers SubscriptionId from vector, that don't exist in our database.
    pub async fn filter_out_known_offer_ids(
        &self,
        offer_ids: Vec<SubscriptionId>,
    ) -> Result<Vec<SubscriptionId>, QueryOffersError> {
        // Make sure we only process ids up to limit from config
        let max_bcasted_offers = self.config.discovery.max_bcasted_offers as usize;
        let offers_idx =
            offer_ids.len() - [offer_ids.len(), max_bcasted_offers].iter().min().unwrap();
        let offer_ids = offer_ids[offers_idx..].to_vec();

        let known_ids = self
            .db
            .as_dao::<OfferDao>()
            .get_known_ids(offer_ids.clone())
            .await?
            .into_iter()
            .collect();

        Ok(offer_ids
            .into_iter()
            .collect::<HashSet<SubscriptionId>>()
            .difference(&known_ids)
            .cloned()
            .collect())
    }

    pub async fn get_offer(&self, id: &SubscriptionId) -> Result<Offer, QueryOfferError> {
        let now = Utc::now().naive_utc();
        match self.db.as_dao::<OfferDao>().get_state(id, now).await {
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
            .unsubscribe(id, Utc::now().naive_utc())
            .await
            .map_err(|e| ModifyOfferError::Unsubscribe(e.into(), id.clone()))
            .and_then(|state| match state {
                OfferState::Active(_) => Ok(()),
                OfferState::NotFound => Err(ModifyOfferError::NotFound(id.clone())),
                OfferState::Unsubscribed(_) => {
                    Err(ModifyOfferError::AlreadyUnsubscribed(id.clone()))
                }
                OfferState::Expired(_) => Err(ModifyOfferError::Expired(id.clone())),
            })
    }

    /// Local Offers are kept after unsubscribe. Offers from other nodes are removed.
    pub async fn unsubscribe_offer(
        &self,
        offer_id: &SubscriptionId,
        local_caller: bool,
        _caller_id: Option<NodeId>,
    ) -> Result<(), ModifyOfferError> {
        // TODO: We can't check caller_id to authorize this operation, because
        //  otherwise we can't get unsubscribe events from other Nodes, than Offer
        //  owner. But on the other side, if we allow anyone to unsubscribe, someone
        //  can use it to attacks. Probably we must ask owner, if he really
        //  unsubscribed his Offer or require owner signatures for all unsubscribes.
        // if let Ok(offer) = self.get_offer(offer_id).await {
        //     if caller_id != Some(offer.node_id) {
        //         // TODO: unauthorized?
        //         return Err(ModifyOfferError::NotFound(offer_id.clone()));
        //     }
        // }

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
            Err(e) => Err(ModifyOfferError::Remove(e, offer_id.clone())),
        }
    }

    pub async fn create_demand(
        &self,
        id: &Identity,
        demand: &NewDemand,
    ) -> Result<Demand, DemandError> {
        let creation_ts = Utc::now().naive_utc();
        // TODO: requestor agent should set expiration.
        let expiration_ts = creation_ts + self.config.subscription.default_ttl;
        let demand = Demand::from_new(demand, &id, creation_ts, expiration_ts)?;
        self.db
            .as_dao::<DemandDao>()
            .insert(&demand)
            .await
            .map_err(|e| DemandError::Save(e))?;
        Ok(demand)
    }

    pub async fn get_demand(&self, id: &SubscriptionId) -> Result<Demand, DemandError> {
        match self.db.as_dao::<DemandDao>().select(id).await {
            Err(e) => Err(DemandError::GetSingle(e, id.clone())),
            Ok(Some(demand)) => Ok(demand),
            Ok(None) => Err(DemandError::NotFound(id.clone())),
        }
    }

    pub async fn get_client_demands(
        &self,
        node_id: Option<NodeId>,
    ) -> Result<Vec<ClientDemand>, QueryDemandsError> {
        Ok(self
            .db
            .as_dao::<DemandDao>()
            .get_demands(node_id, None, Utc::now().naive_utc())
            .await
            .map_err(QueryDemandsError::from)?
            .into_iter()
            .filter_map(|o| match o.into_client_demand() {
                Err(e) => {
                    log::error!("Skipping Demand because of: {}", e);
                    None
                }
                Ok(o) => Some(o),
            })
            .collect())
    }

    pub async fn get_demands_before(
        &self,
        insertion_ts: NaiveDateTime,
    ) -> Result<Vec<Demand>, DemandError> {
        Ok(self
            .db
            .as_dao::<DemandDao>()
            .get_demands(None, Some(insertion_ts), Utc::now().naive_utc())
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
            .map_err(|e| DemandError::Remove(e, demand_id.clone()))?
        {
            true => Ok(()),
            false => Err(DemandError::NotFound(demand_id.clone())),
        }
    }
}
