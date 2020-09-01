use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use ya_client::model::market::{Demand as ClientDemand, Offer as ClientOffer};
use ya_service_api_web::middleware::Identity;

use crate::config::Config;
use crate::db::model::{Demand, Offer, SubscriptionId};
use crate::identity::IdentityApi;
use crate::protocol::discovery::{builder::DiscoveryBuilder, Discovery};

pub(crate) mod cyclic;
pub mod error;
pub(crate) mod handlers;
pub(crate) mod resolver;
pub(crate) mod store;

use error::{MatcherError, MatcherInitError, QueryOffersError};
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
#[derive(Clone)]
pub struct Matcher {
    pub store: SubscriptionStore,
    pub resolver: Resolver,
    discovery: Discovery,
    identity: Arc<dyn IdentityApi>,
    config: Arc<Config>,
}

impl Matcher {
    pub fn new(
        store: SubscriptionStore,
        identity_api: Arc<dyn IdentityApi>,
        config: Arc<Config>,
    ) -> Result<(Matcher, EventsListeners), MatcherInitError> {
        let (proposal_sender, proposal_receiver) = unbounded_channel::<RawProposal>();
        let resolver = Resolver::new(store.clone(), proposal_sender);

        let discovery = DiscoveryBuilder::default()
            .add_data(identity_api.clone())
            .add_data(store.clone())
            .add_data(resolver.clone())
            .add_data_handler(handlers::filter_out_known_offer_ids)
            .add_data_handler(handlers::save_and_match_offers)
            .add_data_handler(handlers::get_offers)
            .add_data_handler(handlers::unsubscribe_offers)
            .build();

        let matcher = Matcher {
            store,
            resolver,
            discovery,
            config,
            identity: identity_api,
        };

        let listeners = EventsListeners { proposal_receiver };
        Ok((matcher, listeners))
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), MatcherInitError> {
        self.discovery
            .bind_gsb(public_prefix, private_prefix)
            .await?;

        // We can't spawn broadcasts, before gsb is bound.
        // That's why we don't spawn this in Matcher::new.
        tokio::task::spawn_local(cyclic::broadcast_offers(self.clone()));
        tokio::task::spawn_local(cyclic::broadcast_unsubscribes(self.clone()));
        Ok(())
    }

    // =========================================== //
    // Offer/Demand subscription
    // =========================================== //

    pub async fn subscribe_offer(
        &self,
        offer: &ClientOffer,
        id: &Identity,
    ) -> Result<Offer, MatcherError> {
        let offer = self.store.create_offer(id, offer).await?;
        self.resolver.receive(&offer);

        log::info!(
            "Subscribed new Offer: [{}] using identity: {} [{}]",
            &offer.id,
            id.name,
            id.identity
        );

        // Ignore error and don't retry to broadcast Offer. It will be broadcasted
        // anyway during random broadcast, so nothing bad happens here in case of error.
        let _ = self
            .discovery
            .broadcast_offers(vec![offer.id.clone()])
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

        log::info!(
            "Unsubscribed Offer: [{}] using identity: {} [{}]",
            &offer_id,
            id.name,
            id.identity
        );

        // Broadcast only, if no Error occurred in previous step.
        // We ignore broadcast errors. Unsubscribing was finished successfully, so:
        // - We shouldn't bother agent with broadcasts errors.
        // - Unsubscribe message probably will reach other markets, but later.
        let _ = self
            .discovery
            .broadcast_unsubscribes(vec![offer_id.clone()])
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

        log::info!(
            "Subscribed new Demand: [{}] using identity: {} [{}]",
            &demand.id,
            id.name,
            id.identity
        );
        Ok(demand)
    }

    pub async fn unsubscribe_demand(
        &self,
        demand_id: &SubscriptionId,
        id: &Identity,
    ) -> Result<(), MatcherError> {
        self.store.remove_demand(demand_id, id).await?;

        log::info!(
            "Unsubscribed Demand: [{}] using identity: {} [{}]",
            &demand_id,
            id.name,
            id.identity
        );
        Ok(())
    }

    pub async fn list_our_offers(&self) -> Result<Vec<Offer>, QueryOffersError> {
        let identities = self.identity.list().await?;
        let store = self.store.clone();

        let mut our_offers = vec![];
        for node_id in identities.into_iter() {
            our_offers.append(&mut store.get_offers(Some(node_id)).await?)
        }

        Ok(our_offers)
    }

    pub async fn list_our_unsubscribed_offers(
        &self,
    ) -> Result<Vec<SubscriptionId>, QueryOffersError> {
        let identities = self.identity.list().await?;
        let store = self.store.clone();

        let mut our_offers = vec![];
        for node_id in identities.into_iter() {
            our_offers.append(&mut store.get_unsubscribed_offers(Some(node_id)).await?)
        }

        Ok(our_offers)
    }
}
