use lazy_static::lazy_static;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::db::models::Offer as ModelOffer;
use crate::db::models::{Demand as ModelDemand, SubscriptionId};
use crate::matcher::{Matcher, MatcherError, MatcherInitError};
use crate::negotiation::{NegotiationError, NegotiationInitError};
use crate::negotiation::{ProviderNegotiationEngine, RequestorNegotiationEngine};

use crate::migrations;
use crate::rest_api;

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
    Negotiation(#[from] NegotiationError),
    #[error("Internal error: {0}.")]
    InternalError(#[from] ErrorMessage),
}

#[derive(Error, Debug)]
pub enum MarketInitError {
    #[error("Failed to initialize Offers matcher. Error: {0}.")]
    Matcher(#[from] MatcherInitError),
    #[error("Failed to initialize negotiation engine. Error: {0}.")]
    Negotiation(#[from] NegotiationInitError),
    #[error("Failed to migrate market database. Error: {0}.")]
    Migration(#[from] anyhow::Error),
}

/// Structure connecting all market objects.
pub struct MarketService {
    pub matcher: Arc<Matcher>,
    pub provider_engine: Arc<ProviderNegotiationEngine>,
    pub requestor_engine: Arc<RequestorNegotiationEngine>,
}

impl MarketService {
    pub fn new(db: &DbExecutor) -> Result<Self, MarketInitError> {
        db.apply_migration(migrations::run_with_output)?;

        let (matcher, listeners) = Matcher::new(db)?;
        let provider_engine = ProviderNegotiationEngine::new(db.clone())?;
        let requestor_engine =
            RequestorNegotiationEngine::new(db.clone(), listeners.proposal_receiver)?;

        Ok(MarketService {
            matcher: Arc::new(matcher),
            provider_engine,
            requestor_engine,
        })
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), MarketInitError> {
        self.matcher.bind_gsb(public_prefix, private_prefix).await?;
        self.provider_engine
            .bind_gsb(public_prefix, private_prefix)
            .await?;
        self.requestor_engine
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
        let offer = ModelOffer::from_new(offer, &id);

        self.matcher.subscribe_offer(&offer).await?;
        self.provider_engine.subscribe_offer(&offer).await?;
        Ok(offer.id)
    }

    pub async fn unsubscribe_offer(
        &self,
        subscription_id: &SubscriptionId,
        id: &Identity,
    ) -> Result<(), MarketError> {
        // TODO: Authorize unsubscribe caller.
        self.provider_engine
            .unsubscribe_offer(subscription_id)
            .await?;
        Ok(self.matcher.unsubscribe_offer(id, subscription_id).await?)
    }

    pub async fn subscribe_demand(
        &self,
        demand: &Demand,
        id: &Identity,
    ) -> Result<SubscriptionId, MarketError> {
        let demand = ModelDemand::from_new(demand, &id);

        self.matcher.subscribe_demand(&demand).await?;
        self.requestor_engine.subscribe_demand(&demand).await?;
        Ok(demand.id)
    }

    pub async fn unsubscribe_demand(
        &self,
        subscription_id: &SubscriptionId,
        id: &Identity,
    ) -> Result<(), MarketError> {
        // TODO: Authorize unsubscribe caller.

        self.requestor_engine
            .unsubscribe_demand(subscription_id)
            .await?;
        Ok(self.matcher.unsubscribe_demand(subscription_id).await?)
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
