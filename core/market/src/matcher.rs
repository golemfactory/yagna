use actix::prelude::*;
use chrono::{TimeZone, Utc};
use metrics::counter;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use ya_client::model::market::{NewDemand, NewOffer};
use ya_service_api_web::middleware::Identity;
use ya_utils_actix::deadline_checker::{
    bind_deadline_reaction, DeadlineChecker, StopTracking, TrackDeadline,
};

use crate::config::Config;
use crate::db::model::{Demand, Offer, SubscriptionId};
use crate::identity::IdentityApi;
use crate::protocol::discovery::{builder::DiscoveryBuilder, Discovery};

pub(crate) mod cyclic;
pub mod error;
pub(crate) mod handlers;
pub(crate) mod resolver;
pub(crate) mod store;

use crate::db::dao::{DemandDao, DemandState};
use error::{MatcherError, MatcherInitError, QueryOfferError, QueryOffersError};
use futures::FutureExt;
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
    expiration_tracker: Addr<DeadlineChecker>,
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
            .add_data_handler(handlers::receive_remote_offers)
            .add_data_handler(handlers::get_local_offers)
            .add_data_handler(handlers::receive_remote_offer_unsubscribes)
            .with_config(config.discovery.clone())
            .build();

        let matcher = Matcher {
            store,
            resolver,
            discovery,
            config,
            identity: identity_api,
            expiration_tracker: DeadlineChecker::new().start(),
        };

        let listeners = EventsListeners { proposal_receiver };

        // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
        // until first change to value will be made.
        counter!("market.offers.incoming", 0);
        counter!("market.offers.broadcasts", 0);
        counter!("market.offers.broadcasts.skip", 0);
        counter!("market.offers.broadcasts.net", 0);
        counter!("market.offers.broadcasts.net_errors", 0);
        counter!("market.offers.unsubscribes.incoming", 0);
        counter!("market.offers.unsubscribes.broadcasts", 0);
        counter!("market.offers.unsubscribes.broadcasts.net", 0);
        counter!("market.offers.unsubscribes.broadcasts.net_errors", 0);

        Ok((matcher, listeners))
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        local_prefix: &str,
    ) -> Result<(), MatcherInitError> {
        self.discovery.bind_gsb(public_prefix, local_prefix).await?;

        // We can't spawn broadcasts, before gsb is bound.
        // That's why we don't spawn this in Matcher::new.
        tokio::task::spawn_local(cyclic::bcast_offers(self.clone()));
        tokio::task::spawn_local(cyclic::bcast_unsubscribes(self.clone()));

        self.bind_expiration_tracker()
            .await
            .map_err(|e| MatcherInitError::ExpirationTrackerError(e.to_string()))?;
        Ok(())
    }

    pub async fn bind_expiration_tracker(&self) -> anyhow::Result<()> {
        let store = self.store.clone();
        bind_deadline_reaction(self.expiration_tracker.clone(), move |msg| {
            let store = store.clone();
            async move {
                let id = SubscriptionId::from_str(&msg.id);
                match (&msg.category[..], &id) {
                    ("Offer", Ok(id)) => match store.get_offer(id).await {
                        Err(QueryOfferError::Expired(_)) => {
                            log::info!("Offer [{}] expired.", id);
                            counter!("market.offers.expired", 1)
                        }
                        _ => (),
                    },
                    ("Demand", Ok(id)) => {
                        match store.db.as_dao::<DemandDao>().demand_state(id).await {
                            Ok(DemandState::Expired(_)) => {
                                log::info!("Demand [{}] expired.", id);
                                counter!("market.demands.expired", 1)
                            }
                            _ => (),
                        }
                    }
                    _ => {}
                }
            }
            .boxed()
        })
        .await?;
        Ok(())
    }

    // =========================================== //
    // Offer/Demand subscription
    // =========================================== //

    pub async fn subscribe_offer(
        &self,
        offer: &NewOffer,
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

        self.expiration_tracker
            .send(TrackDeadline {
                category: "Offer".to_string(),
                deadline: Utc.from_utc_datetime(&offer.expiration_ts),
                id: offer.id.to_string(),
            })
            .await
            .ok();

        // Ignore error and don't retry to broadcast Offer. It will be broadcasted
        // anyway during random broadcast, so nothing bad happens here in case of error.
        let _ = self
            .discovery
            .bcast_offers(vec![offer.id.clone()])
            .await
            .map_err(|e| {
                log::warn!("Failed to bcast offer [{}]. Error: {}.", offer.id, e,);
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

        self.expiration_tracker
            .send(StopTracking {
                category: Some("Offer".to_string()),
                id: offer_id.to_string(),
            })
            .await
            .ok();

        // Broadcast only, if no Error occurred in previous step.
        // We ignore broadcast errors. Unsubscribing was finished successfully, so:
        // - We shouldn't bother agent with broadcasts errors.
        // - Unsubscribe message probably will reach other markets, but later.
        let _ = self
            .discovery
            .bcast_unsubscribes(vec![offer_id.clone()])
            .await
            .map_err(|e| {
                log::warn!(
                    "Failed to bcast unsubscribe offer [{1}]. Error: {0}.",
                    e,
                    offer_id
                );
            });
        Ok(())
    }

    pub async fn subscribe_demand(
        &self,
        demand: &NewDemand,
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

    pub async fn get_our_active_offer_ids(&self) -> Result<Vec<SubscriptionId>, QueryOffersError> {
        let our_node_ids = self.identity.list().await?;
        Ok(self.store.get_active_offer_ids(Some(our_node_ids)).await?)
    }

    pub async fn get_our_unsubscribed_offer_ids(
        &self,
    ) -> Result<Vec<SubscriptionId>, QueryOffersError> {
        let our_node_ids = self.identity.list().await?;
        Ok(self
            .store
            .get_unsubscribed_offer_ids(Some(our_node_ids))
            .await?)
    }
}
