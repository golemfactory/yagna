use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use ya_client::model::market::{Demand as ClientDemand, Offer as ClientOffer};
use ya_service_api_web::middleware::Identity;

use crate::db::model::{Demand, Offer, SubscriptionId};
use crate::protocol::discovery::builder::DiscoveryBuilder;
use crate::protocol::discovery::{
    Discovery, OfferReceived, OfferUnsubscribed, Propagate, Reason, RetrieveOffers,
};

pub mod error;
pub(crate) mod resolver;
pub(crate) mod store;

use error::{MatcherError, MatcherInitError, ModifyOfferError, SaveOfferError};
use resolver::Resolver;
use store::SubscriptionStore;

/// Stores proposal generated from resolver.
#[derive(Debug)]
pub struct RawProposal {
    pub offer: Offer,
    pub demand: Demand,
}

/// Receivers for events, that can be emitted from Matcher.
pub struct EventsListeners {
    pub proposal_receiver: UnboundedReceiver<RawProposal>,
}

/// Responsible for storing Offers and matching them with demands.
pub struct Matcher {
    pub store: SubscriptionStore,
    pub resolver: Resolver,
    discovery: Discovery,
}

impl Matcher {
    pub fn new(store: SubscriptionStore) -> Result<(Matcher, EventsListeners), MatcherInitError> {
        let (proposal_sender, proposal_receiver) = unbounded_channel::<RawProposal>();
        let resolver = Resolver::new(store.clone(), proposal_sender);

        let discovery = DiscoveryBuilder::default()
            .data(store.clone())
            .data(resolver.clone())
            .add_data_handler(on_offer_received)
            .add_data_handler(on_offer_unsubscribed)
            .add_handler(move |caller: String, _msg: RetrieveOffers| async move {
                log::info!("Offers request received from: {}. Unimplemented.", caller);
                Ok(vec![])
            })
            .build();

        let matcher = Matcher {
            store,
            resolver,
            discovery,
        };

        let listeners = EventsListeners { proposal_receiver };

        Ok((matcher, listeners))
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), MatcherInitError> {
        Ok(self
            .discovery
            .bind_gsb(public_prefix, private_prefix)
            .await?)
    }

    // =========================================== //
    // Offer/Demand subscription
    // =========================================== //

    pub async fn subscribe_offer(
        &self,
        offer: &ClientOffer,
        id: &Identity,
    ) -> Result<Offer, MatcherError> {
        // TODO: Handle broadcast errors. Maybe we should retry if it failed.
        let offer = self.store.create_offer(id, offer).await?;
        self.resolver.receive(&offer);

        let _ = self
            .discovery
            .broadcast_offer(offer.clone())
            .await
            .map_err(|e| {
                log::warn!("Failed to broadcast offer [{}]. Error: {}.", offer.id, e,);
            });
        Ok(offer)
    }

    pub async fn unsubscribe_offer(
        &self,
        offer_id: &SubscriptionId,
        id: &Identity,
    ) -> Result<(), MatcherError> {
        self.store
            .unsubscribe_offer(offer_id, true, Some(id.identity))
            .await?;

        // Broadcast only, if no Error occurred in previous step.
        // We ignore broadcast errors. Unsubscribing was finished successfully, so:
        // - We shouldn't bother agent with broadcasts
        // - Unsubscribe message probably will reach other markets, but later.
        let _ = self
            .discovery
            .broadcast_unsubscribe(id.identity.to_string(), offer_id.clone())
            .await
            .map_err(|e| {
                log::warn!(
                    "Failed to broadcast unsubscribe offer [{1}]. Error: {0}.",
                    e,
                    offer_id
                );
            });
        Ok(())
    }

    pub async fn subscribe_demand(
        &self,
        demand: &ClientDemand,
        id: &Identity,
    ) -> Result<Demand, MatcherError> {
        let demand = self.store.create_demand(id, demand).await?;

        self.resolver.receive(&demand);
        Ok(demand)
    }

    pub async fn unsubscribe_demand(
        &self,
        demand_id: &SubscriptionId,
        id: &Identity,
    ) -> Result<(), MatcherError> {
        Ok(self.store.remove_demand(demand_id, id).await?)
    }
}

pub(crate) async fn on_offer_received(
    resolver: Resolver,
    _caller: String,
    msg: OfferReceived,
) -> Result<Propagate, ()> {
    // We shouldn't propagate Offer, if we already have it in our database.
    // Note that when we broadcast our Offer, it will reach us too, so it concerns
    // not only Offers from other nodes.

    resolver
        .store
        .save_offer(msg.offer)
        .await
        .map(|offer| {
            resolver.receive(&offer);
            Propagate::Yes
        })
        .or_else(|e| match e {
            // Stop propagation for existing, unsubscribed and expired Offers to avoid infinite broadcast.
            SaveOfferError::Exists(_) => Ok(Propagate::No(Reason::AlreadyExists)),
            SaveOfferError::Unsubscribed(_) => Ok(Propagate::No(Reason::Unsubscribed)),
            SaveOfferError::Expired(_) => Ok(Propagate::No(Reason::Expired)),
            // Below errors are not possible to get from checked_store_offer
            SaveOfferError::SaveError(_, _)
            | SaveOfferError::SubscriptionValidation(_)
            | SaveOfferError::WrongState { .. } => {
                Ok(Propagate::No(Reason::Error(format!("{}", e))))
            }
        })
}

pub(crate) async fn on_offer_unsubscribed(
    store: SubscriptionStore,
    caller: String,
    msg: OfferUnsubscribed,
) -> Result<Propagate, ()> {
    store
        .unsubscribe_offer(&msg.offer_id, false, caller.parse().ok())
        .await
        .map(|_| Propagate::Yes)
        .or_else(|e| match e {
            ModifyOfferError::UnsubscribeError(_, _)
            | ModifyOfferError::UnsubscribedNotRemoved(_)
            | ModifyOfferError::RemoveError(_, _) => {
                log::error!("Propagating Offer unsubscription, despite error: {}", e);
                // TODO: how should we handle it locally?
                Ok(Propagate::Yes)
            }
            ModifyOfferError::NotFound(_) => Ok(Propagate::No(Reason::NotFound)),
            ModifyOfferError::Unsubscribed(_) => Ok(Propagate::No(Reason::Unsubscribed)),
            ModifyOfferError::Expired(_) => Ok(Propagate::No(Reason::Expired)),
        })
}
