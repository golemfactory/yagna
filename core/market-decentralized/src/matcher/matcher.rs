use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use ya_client_model::market::{Demand, Offer, Proposal};
use ya_persistence::executor::DbExecutor;
use ya_persistence::executor::Error as DbError;

use crate::protocol::{Discovery, DiscoveryBuilder, DiscoveryFactory, DiscoveryInitError};

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
    ) -> Result<(Matcher, EventsListeners), MatcherInitError> {
        // TODO: Bind Discovery callbacks.
        let discovery = Factory::new(builder)?;

        // TODO: Create new database for offers.
        let db = DbExecutor::new("[Separate database for offers]")?;

        let (emitter, receiver) = unbounded_channel::<Proposal>();

        let matcher = Matcher {
            db,
            discovery,
            proposal_emitter: emitter,
        };
        let listeners = EventsListeners {
            proposal_receiver: receiver,
        };

        Ok((matcher, listeners))
    }

    async fn add_offer(&self, offer: Offer) {
        unimplemented!();
    }

    async fn subscribe_offer(&self, offer: Offer) {
        unimplemented!();
    }

    async fn subscribe_demand(&self, subscription_id: String) {
        unimplemented!();
    }

    async fn unsubscribe_offer(&self, offer: Demand) {
        unimplemented!();
    }

    async fn unsubscribe_demand(&self, subscription_id: String) {
        unimplemented!();
    }
}
