use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use ya_client::model::market::{Demand as ClientDemand, Offer as ClientOffer};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::db::models::{Demand, Offer};
use crate::protocol::{
    Discovery, DiscoveryBuilder, OfferReceived, OfferUnsubscribed, Propagate, Reason,
    RetrieveOffers,
};
use crate::SubscriptionId;

pub mod error;
pub mod resolver;
pub mod store;

pub use error::{DemandError, MatcherError, MatcherInitError, OfferError};
use resolver::{Resolver, Subscription};
pub use store::SubscriptionStore;

/// Stores proposal generated from resolver.
#[derive(Debug)]
pub struct RawProposal {
    pub offer: Offer,
    pub demand: Demand,
}

/// Receivers for events, that can be emitted from Matcher.
pub struct EventsListeners {
    pub proposal_rx: UnboundedReceiver<RawProposal>,
}

/// Responsible for storing Offers and matching them with demands.
pub struct Matcher {
    pub store: SubscriptionStore,
    pub resolver: Resolver,
    discovery: Discovery,
}

impl Matcher {
    pub fn new(db: &DbExecutor) -> Result<(Matcher, EventsListeners), MatcherInitError> {
        let store = SubscriptionStore::new(db.clone());
        let (proposal_tx, proposal_rx) = unbounded_channel::<RawProposal>();
        let resolver = Resolver::new(store.clone(), proposal_tx);

        let discovery = DiscoveryBuilder::default()
            .data(store.clone())
            .data(resolver.clone())
            .add_data_handler(on_offer_received)
            .add_data_handler(on_offer_unsubscribed)
            .add_handler(move |caller: String, msg: RetrieveOffers| async move {
                log::info!("Offers request received from: {}. Unimplemented.", caller);
                Ok(vec![])
            })
            .build();

        let (emitter, receiver) = unbounded_channel::<RawProposal>();

        let matcher = Matcher {
            store,
            resolver,
            discovery,
        };

        let listeners = EventsListeners { proposal_rx };

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
        // TODO: Run matching to find local matching demands. We shouldn't wait here.
        // TODO: Handle broadcast errors. Maybe we should retry if it failed.
        let offer = self.store.create_offer(id, offer).await?;
        self.resolver.receive(&offer)?;

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
        id: &Identity,
        subscription_id: &SubscriptionId,
    ) -> Result<(), MatcherError> {
        self.store.mark_offer_unsubscribed(subscription_id).await?;

        // Broadcast only, if no Error occurred in previous step.
        // We ignore broadcast errors. Unsubscribing was finished successfully, so:
        // - We shouldn't bother agent with broadcasts
        // - Unsubscribe message probably will reach other markets, but later.
        let _ = self
            .discovery
            .broadcast_unsubscribe(id.identity.to_string(), subscription_id.clone())
            .await
            .map_err(|e| {
                log::warn!(
                    "Failed to broadcast unsubscribe offer [{1}]. Error: {0}.",
                    e,
                    subscription_id
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

        self.resolver.receive(&demand)?;
        Ok(demand)
    }

    pub async fn unsubscribe_demand(
        &self,
        _id: &Identity,
        subscription_id: &SubscriptionId,
    ) -> Result<(), MatcherError> {
        Ok(self.store.remove_demand(subscription_id).await?)
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

    let subscription = Subscription::from(&msg.offer);
    resolver
        .store
        .store_offer(msg.offer)
        .await
        .map(|propagate| match propagate {
            true => {
                resolver.receive(subscription).unwrap();
                Propagate::Yes
            }
            false => Propagate::No(Reason::AlreadyExists),
        })
        .or_else(|e| match e {
            // Stop propagation for expired and unsubscribed Offers to avoid infinite broadcast.
            OfferError::AlreadyUnsubscribed(_) => Ok(Propagate::No(Reason::Unsubscribed)),
            OfferError::Expired(_) => Ok(Propagate::No(Reason::Expired)),
            // Below errors are not possible to get from checked_store_offer
            OfferError::NotFound(_)
            | OfferError::UnsubscribeError(_, _)
            | OfferError::GetMany(_)
            | OfferError::RemoveError(_, _)
            | OfferError::UnexpectedError(_) => {
                log::error!("Unexpected error handling offer reception: {}.", e);
                panic!("Should not happened: {}.", e)
            }
            OfferError::SaveError(_, _)
            | OfferError::GetError(_, _)
            | OfferError::SubscriptionValidation(_) => {
                Ok(Propagate::No(Reason::Error(format!("{}", e))))
            }
        })
}

pub(crate) async fn on_offer_unsubscribed(
    store: SubscriptionStore,
    _caller: String,
    msg: OfferUnsubscribed,
) -> Result<Propagate, ()> {
    store
        .remove_offer(&msg.subscription_id)
        .await
        .map(|_| Propagate::Yes)
        .or_else(|e| match e {
            OfferError::UnsubscribeError(_, _)
            | OfferError::RemoveError(_, _)
            | OfferError::UnexpectedError(_) => {
                log::error!("Propagating Offer unsubscription, while error: {}", e);
                // TODO: how should we handle it locally?
                Ok(Propagate::Yes)
            }
            OfferError::NotFound(_) => Ok(Propagate::No(Reason::NotFound)),
            OfferError::AlreadyUnsubscribed(_) => Ok(Propagate::No(Reason::Unsubscribed)),
            OfferError::Expired(_) => Ok(Propagate::No(Reason::Expired)),
            OfferError::SaveError(_, _)
            | OfferError::GetError(_, _)
            | OfferError::GetMany(_)
            | OfferError::SubscriptionValidation(_) => {
                log::error!("Unexpected error handling offer unsubscription: {}.", e);
                panic!("Should not happened: {}.", e)
            }
        })
}
