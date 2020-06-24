use tokio::sync::mpsc::{error::SendError, unbounded_channel, UnboundedReceiver, UnboundedSender};

use ya_client::model::market::Proposal;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::db::models::Offer;
use crate::protocol::{Discovery, OfferReceived, OfferUnsubscribed, RetrieveOffers};
use crate::SubscriptionId;

use crate::matcher::handlers::{on_offer_received, on_offer_unsubscribed};
pub use error::{DemandError, MatcherError, MatcherInitError, OfferError};
pub use store::SubscriptionStore;

mod error;
mod handlers;
mod resolver;
mod store;

/// Receivers for events, that can be emitted from Matcher.
pub struct EventsListeners {
    pub proposal_receiver: UnboundedReceiver<Proposal>,
}

/// Responsible for storing Offers and matching them with demands.
#[derive(Clone)]
pub struct Matcher {
    db: DbExecutor,
    discovery: Discovery,
    proposal_emitter: UnboundedSender<Proposal>,
}

impl Matcher {
    pub fn new(db: &DbExecutor) -> Result<(Matcher, EventsListeners), MatcherInitError> {
        // TODO: Implement Discovery callbacks.

        let database1 = db.clone();
        let database2 = db.clone();
        let discovery = Discovery::new(
            move |caller: String, msg: OfferReceived| {
                let database = database1.clone();
                on_offer_received(database, caller, msg)
            },
            move |caller: String, msg: OfferUnsubscribed| {
                let database = database2.clone();
                on_offer_unsubscribed(database, caller, msg)
            },
            move |caller: String, msg: RetrieveOffers| async move {
                log::info!("Offers request received from: {}. Unimplemented.", caller);
                Ok(vec![])
            },
        )?;
        let (emitter, receiver) = unbounded_channel::<Proposal>();

        let matcher = Matcher {
            db: db.clone(),
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

    pub async fn subscribe_offer(&self, offer: &Offer) -> Result<(), MatcherError> {
        // TODO: Run matching to find local matching demands. We shouldn't wait here.
        // TODO: Handle broadcast errors. Maybe we should retry if it failed.
        let _ = self
            .discovery
            .broadcast_offer(offer.clone())
            .await
            .map_err(|e| {
                log::warn!("Failed to broadcast offer [{}]. Error: {}.", offer.id, e,);
            });
        Ok(())
    }

    pub async fn unsubscribe_offer(
        &self,
        id: &Identity,
        subscription_id: &SubscriptionId,
    ) -> Result<(), MatcherError> {
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

    pub fn emit_proposal(&self, proposal: Proposal) -> Result<(), SendError<Proposal>> {
        self.proposal_emitter.send(proposal)
    }
}
