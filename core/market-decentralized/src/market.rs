use std::sync::Arc;
use thiserror::Error;

use crate::matcher::{Matcher, MatcherInitError};
use crate::negotiation::{ProviderNegotiationEngine, RequestorNegotiationEngine};
use crate::negotiation::NegotiationError;
use crate::protocol::{DiscoveryBuilder, DiscoveryGSB};

use ya_persistence::executor::DbExecutor;
use ya_persistence::executor::Error as DbError;

#[derive(Error, Debug)]
pub enum MarketError {}

#[derive(Error, Debug)]
pub enum MarketInitError {
    #[error("Failed to initialize Offers matcher. Error: {}.", .0)]
    MatcherError(#[from] MatcherInitError),
    #[error("Failed to initialize database. Error: {}.", .0)]
    DatabaseError(#[from] DbError),
    #[error("Failed to initialize negotiation engine. Error: {}.", .0)]
    NegotiationError(#[from] NegotiationError),
}


/// Structure connecting all market objects.
pub struct Market {
    matcher: Arc<Matcher>,
    provider_negotiation_engine: Arc<ProviderNegotiationEngine>,
    requestor_negotiation_engine: Arc<RequestorNegotiationEngine>,
}

impl Market {
    pub fn new() -> Result<Self, MarketInitError> {
        // TODO: Set Matcher independent parameters here or remove this todo.
        let builder = DiscoveryBuilder::new();

        let db = DbExecutor::new("[common url for all apis]")?;

        let (matcher, listeners) = Matcher::new::<DiscoveryGSB>(builder)?;
        let provider_engine = ProviderNegotiationEngine::new(db.clone())?;
        let requestor_engine = RequestorNegotiationEngine::new(
            db,
            listeners.proposal_receiver,
        )?;

        Ok(Market {
            matcher: Arc::new(matcher),
            provider_negotiation_engine: provider_engine,
            requestor_negotiation_engine: requestor_engine,
        })
    }
}
