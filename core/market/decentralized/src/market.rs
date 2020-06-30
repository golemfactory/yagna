use lazy_static::lazy_static;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::matcher::error::{DemandError, MatcherError, MatcherInitError, OfferError};
use crate::matcher::Matcher;
use crate::negotiation::{NegotiationError, NegotiationInitError};
use crate::negotiation::{ProviderNegotiationEngine, RequestorNegotiationEngine};
use crate::rest_api;
use crate::{migrations, SubscriptionId};

use ya_client::model::market::{Demand, Offer};
use ya_client::model::ErrorMessage;
use ya_core_model::market::{private, BUS_ID};
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};
use ya_service_api_web::middleware::Identity;
use ya_service_api_web::scope::ExtendableScope;

#[derive(Error, Debug)]
pub enum MarketError {
    #[error(transparent)]
    Matcher(#[from] MatcherError),
    #[error(transparent)]
    OfferError(#[from] OfferError),
    #[error(transparent)]
    DemandError(#[from] DemandError),
    #[error(transparent)]
    Negotiation(#[from] NegotiationError),
    #[error("Internal error: {0}.")]
    InternalError(#[from] ErrorMessage),
}

#[derive(Error, Debug)]
pub enum MarketInitError {
    #[error("Failed to initialize Matcher. Error: {0}.")]
    Matcher(#[from] MatcherInitError),
    #[error("Failed to initialize negotiation engine. Error: {0}.")]
    Negotiation(#[from] NegotiationInitError),
    #[error("Failed to migrate market database. Error: {0}.")]
    Migration(#[from] anyhow::Error),
}

/// Structure connecting all market objects.
pub struct MarketService {
    pub matcher: Matcher,
    pub provider_negotiation_engine: ProviderNegotiationEngine,
    pub requestor_negotiation_engine: RequestorNegotiationEngine,
}

impl MarketService {
    pub fn new(db: &DbExecutor) -> Result<Self, MarketInitError> {
        db.apply_migration(migrations::run_with_output)?;

        let (matcher, listeners) = Matcher::new(db)?;
        let provider_engine = ProviderNegotiationEngine::new(db.clone())?;
        let requestor_engine = RequestorNegotiationEngine::new(db.clone(), listeners.proposal_rx)?;

        Ok(MarketService {
            matcher,
            provider_negotiation_engine: provider_engine,
            requestor_negotiation_engine: requestor_engine,
        })
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), MarketInitError> {
        self.matcher.bind_gsb(public_prefix, private_prefix).await?;
        self.provider_negotiation_engine
            .bind_gsb(public_prefix, private_prefix)
            .await?;
        self.requestor_negotiation_engine
            .bind_gsb(public_prefix, private_prefix)
            .await?;
        Ok(())
    }

    pub async fn gsb<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> anyhow::Result<()> {
        let market = MARKET.get_or_init_market(&ctx.component())?;
        Ok(market.bind_gsb(BUS_ID, private::BUS_ID).await?)
    }

    pub fn rest<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> actix_web::Scope {
        let market = match MARKET.get_or_init_market(&ctx.component()) {
            Ok(market) => market,
            Err(e) => {
                log::error!("REST API initialization failed: {}", e);
                panic!("Market Service initialization impossible: {}", e)
            }
        };
        MarketService::bind_rest(market)
    }

    pub fn bind_rest(myself: Arc<MarketService>) -> actix_web::Scope {
        actix_web::web::scope(crate::MARKET_API_PATH)
            .data(myself)
            .app_data(rest_api::path_config())
            .extend(rest_api::provider::register_endpoints)
            .extend(rest_api::requestor::register_endpoints)
    }

    pub async fn subscribe_offer(
        &self,
        offer: &Offer,
        id: &Identity,
    ) -> Result<SubscriptionId, MarketError> {
        let offer = self.matcher.subscribe_offer(id, offer).await?;
        self.provider_negotiation_engine
            .subscribe_offer(&offer)
            .await?;
        Ok(offer.id)
    }

    pub async fn unsubscribe_offer(
        &self,
        subscription_id: &SubscriptionId,
        id: &Identity,
    ) -> Result<(), MarketError> {
        // TODO: Authorize unsubscribe caller.
        self.provider_negotiation_engine
            .unsubscribe_offer(subscription_id)
            .await?;
        Ok(self.matcher.unsubscribe_offer(id, subscription_id).await?)
    }

    pub async fn subscribe_demand(
        &self,
        demand: &Demand,
        id: &Identity,
    ) -> Result<SubscriptionId, MarketError> {
        let demand = self.matcher.subscribe_demand(id, demand).await?;
        self.requestor_negotiation_engine
            .subscribe_demand(&demand)
            .await?;
        Ok(demand.id)
    }

    pub async fn unsubscribe_demand(
        &self,
        subscription_id: &SubscriptionId,
        id: &Identity,
    ) -> Result<(), MarketError> {
        // TODO: Authorize unsubscribe caller.
        self.requestor_negotiation_engine
            .unsubscribe_demand(subscription_id)
            .await?;
        // TODO: shouldn't remove precede negotiation unsubscribe?
        Ok(self.matcher.unsubscribe_demand(id, subscription_id).await?)
    }
}

impl Service for MarketService {
    type Cli = ();
}

// =========================================== //
// Awful static initialization. Necessary to
// share Market between gsb and rest functions.
// =========================================== //

struct StaticMarket {
    locked_market: Mutex<Option<Arc<MarketService>>>,
}

impl StaticMarket {
    pub fn new() -> StaticMarket {
        StaticMarket {
            locked_market: Mutex::new(None),
        }
    }

    pub fn get_or_init_market(
        &self,
        db: &DbExecutor,
    ) -> Result<Arc<MarketService>, MarketInitError> {
        let mut guarded_market = self.locked_market.lock().unwrap();
        if let Some(market) = &*guarded_market {
            Ok(market.clone())
        } else {
            let market = Arc::new(MarketService::new(db)?);
            *guarded_market = Some(market.clone());
            Ok(market)
        }
    }
}

lazy_static! {
    static ref MARKET: StaticMarket = StaticMarket::new();
}
