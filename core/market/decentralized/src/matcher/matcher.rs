use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use ya_client::model::market::{Demand, Offer, Proposal};
use ya_persistence::executor::DbExecutor;
use ya_persistence::executor::Error as DbError;

use crate::protocol::{Discovery, DiscoveryBuilder, DiscoveryFactory, DiscoveryInitError};
use crate::protocol::{OfferReceived, RetrieveOffers};

#[derive(Error, Debug)]
pub enum MatcherError {}

#[derive(Error, Debug)]
pub enum MatcherInitError {
    #[error("Failed to initialize Discovery interface. Error: {}.", .0)]
    DiscoveryError(#[from] DiscoveryInitError),
    #[error("Failed to initialize database. Error: {}.", .0)]
    DatabaseError(#[from] DbError),
}

/// Receivers for events, that can be emitted from Matcher.
pub struct EventsListeners {
    pub proposal_receiver: UnboundedReceiver<Proposal>,
}

/// Responsible for storing Offers and matching them with demands.
pub struct Matcher {
    db: DbExecutor,
    discovery: Arc<dyn Discovery>,
    proposal_emitter: UnboundedSender<Proposal>,
}

impl Matcher {
    pub fn new<Factory: DiscoveryFactory>(
        builder: DiscoveryBuilder,
        db: &DbExecutor,
    ) -> Result<(Matcher, EventsListeners), MatcherInitError> {
        // TODO: Implement Discovery callbacks.
        let builder = builder
            .bind_offer_received(move |msg: OfferReceived| async move {
                log::info!("Offer from [{}] received.", msg.offer.offer_id.unwrap());
                Ok(())
            })
            .bind_retrieve_offers(move |msg: RetrieveOffers| async move {
                log::info!("Offers request received.");
                Ok(vec![])
            });

        let discovery = Factory::new(builder)?;

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

    pub async fn bind_gsb(&self, prefix: String) -> Result<(), MatcherInitError> {
        Ok(self.discovery.bind_gsb(prefix).await?)
    }

    pub async fn add_offer(&self, offer: Offer) {
        unimplemented!();
    }

    pub async fn subscribe_offer(&self, offer: &Offer) -> Result<(), MatcherError> {
        unimplemented!();
    }

    pub async fn subscribe_demand(&self, demand: &Demand) -> Result<(), MatcherError> {
        unimplemented!();
    }

    pub async fn unsubscribe_offer(&self, subscription_id: String) -> Result<(), MatcherError> {
        unimplemented!();
    }

    pub async fn unsubscribe_demand(&self, subscription_id: String) -> Result<(), MatcherError> {
        unimplemented!();
    }
}
