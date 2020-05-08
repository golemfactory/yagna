use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc::{channel, Receiver, Sender};

use ya_client_model::market::{Demand, Offer, Proposal};
use ya_persistence::executor::DbExecutor;

use crate::protocol::Discovery;

#[derive(Error, Debug)]
pub enum MatcherError {}

/// Receivers for events, that can be emitted from Matcher.
pub struct Emitters {
    proposal_emitter: Receiver<Proposal>,
}

/// Responsible for storing Offers and matching them with demands.
pub struct Matcher {
    db: DbExecutor,
    discovery: Arc<dyn Discovery>,
    proposal_emitter: Sender<Proposal>,
}

impl Matcher {
    pub fn new() -> Result<(Matcher, Emitters), MatcherError> {
        unimplemented!();
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
