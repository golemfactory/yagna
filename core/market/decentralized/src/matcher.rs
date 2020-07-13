use tokio::sync::mpsc::{error::SendError, unbounded_channel, UnboundedReceiver, UnboundedSender};

use ya_client::model::market::{Demand as ClientDemand, Offer as ClientOffer};
use ya_service_api_web::middleware::Identity;

use crate::db::models::{Demand, Offer};
use crate::protocol::{
    Discovery, OfferReceived, OfferUnsubscribed, Propagate, Reason, RetrieveOffers,
};
use crate::SubscriptionId;

pub use error::{DemandError, MatcherError, MatcherInitError, OfferError};
pub use store::SubscriptionStore;

mod error;
mod resolver;
mod store;

/// Stores proposal generated from resolver.
#[derive(Debug)]
pub struct DraftProposal {
    pub offer: Offer,
    pub demand: Demand,
}

/// Receivers for events, that can be emitted from Matcher.
pub struct EventsListeners {
    pub proposal_receiver: UnboundedReceiver<DraftProposal>,
}

/// Responsible for storing Offers and matching them with demands.
pub struct Matcher {
    pub store: SubscriptionStore,
    discovery: Discovery,
    proposal_emitter: UnboundedSender<DraftProposal>,
}

impl Matcher {
    pub fn new(store: SubscriptionStore) -> Result<(Matcher, EventsListeners), MatcherInitError> {
        let store1 = store.clone();
        let store2 = store.clone();
        let discovery = Discovery::new(
            move |caller: String, msg: OfferReceived| {
                let store = store1.clone();
                on_offer_received(store, caller, msg)
            },
            move |caller: String, msg: OfferUnsubscribed| {
                let store = store2.clone();
                on_offer_unsubscribed(store, caller, msg)
            },
            move |caller: String, msg: RetrieveOffers| async move {
                log::info!("Offers request received from: {}. Unimplemented.", caller);
                Ok(vec![])
            },
        )?;
        let (emitter, receiver) = unbounded_channel::<DraftProposal>();

        let matcher = Matcher {
            store,
            discovery,
            proposal_emitter: emitter,
        };

        let listeners = EventsListeners {
            proposal_receiver: receiver,
        };

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
        id: &Identity,
        offer: &ClientOffer,
    ) -> Result<Offer, MatcherError> {
        // TODO: Run matching to find local matching demands. We shouldn't wait here.
        // TODO: Handle broadcast errors. Maybe we should retry if it failed.
        let offer = self.store.create_offer(id, offer).await?;

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
        id: &Identity,
        demand: &ClientDemand,
    ) -> Result<Demand, MatcherError> {
        let demand = self.store.create_demand(id, demand).await?;

        // TODO: Try to match demand with offers currently existing in database.
        //  We shouldn't await here on this.
        Ok(demand)
    }

    pub async fn unsubscribe_demand(
        &self,
        _id: &Identity,
        subscription_id: &SubscriptionId,
    ) -> Result<(), MatcherError> {
        Ok(self.store.remove_demand(subscription_id).await?)
    }

    pub fn emit_proposal(&self, proposal: DraftProposal) -> Result<(), SendError<DraftProposal>> {
        self.proposal_emitter.send(proposal)
    }
}

pub(crate) async fn on_offer_received(
    store: SubscriptionStore,
    _caller: String,
    msg: OfferReceived,
) -> Result<Propagate, ()> {
    // We shouldn't propagate Offer, if we already have it in our database.
    // Note that when we broadcast our Offer, it will reach us too, so it concerns
    // not only Offers from other nodes.

    store
        .store_offer(msg.offer)
        .await
        .map(|propagate| match propagate {
            true => Propagate::Yes,
            false => Propagate::No(Reason::AlreadyExists),
        })
        .or_else(|e| match e {
            // Stop propagation for expired and unsubscribed Offers to avoid infinite broadcast.
            OfferError::AlreadyUnsubscribed(_) => Ok(Propagate::No(Reason::Unsubscribed)),
            OfferError::Expired(_) => Ok(Propagate::No(Reason::Expired)),
            // Below errors are not possible to get from checked_store_offer
            OfferError::NotFound(_)
            | OfferError::UnsubscribeError(_, _)
            | OfferError::RemoveError(_, _)
            | OfferError::UnexpectedError(_) => {
                log::error!("Unexpected error handling offer reception: {}.", e);
                panic!("Should not happened: {}.", e)
            }
            _ => Ok(Propagate::No(Reason::Error(format!("{}", e)))),
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
            | OfferError::SubscriptionValidation(_) => {
                log::error!("Unexpected error handling offer unsubscription: {}.", e);
                panic!("Should not happened: {}.", e)
            }
        })
}
