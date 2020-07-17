use lazy_static::lazy_static;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::db::model::SubscriptionId;
use crate::matcher::error::{
    DemandError, MatcherError, MatcherInitError, QueryOfferError, QueryOffersError,
};
use crate::matcher::{store::SubscriptionStore, Matcher};
use crate::negotiation::error::{NegotiationError, NegotiationInitError};
use crate::negotiation::{ProviderBroker, RequestorBroker};

use crate::rest_api;

use ya_client::model::market::{Demand, Offer};
use ya_client::model::ErrorMessage;
use ya_core_model::market::{private, BUS_ID};
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};
use ya_service_api_web::middleware::Identity;
use ya_service_api_web::scope::ExtendableScope;

pub mod agreement;

#[derive(Error, Debug)]
pub enum MarketError {
    #[error(transparent)]
    Matcher(#[from] MatcherError),
    #[error(transparent)]
    QueryOfferError(#[from] QueryOfferError),
    #[error(transparent)]
    QueryOffersError(#[from] QueryOffersError),
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
    pub db: DbExecutor,
    pub matcher: Matcher,
    pub provider_engine: ProviderBroker,
    pub requestor_engine: RequestorBroker,
}

impl MarketService {
    pub fn new(db: &DbExecutor) -> Result<Self, MarketInitError> {
        db.apply_migration(crate::db::migrations::run_with_output)?;

        let store = SubscriptionStore::new(db.clone());
        let (matcher, listeners) = Matcher::new(store.clone())?;
        let provider_engine = ProviderBroker::new(db.clone(), store.clone())?;
        let requestor_engine =
            RequestorBroker::new(db.clone(), store.clone(), listeners.proposal_receiver)?;

        Ok(MarketService {
            db: db.clone(),
            matcher,
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
        agreement::bind_gsb(self.db.clone(), public_prefix, private_prefix).await;
        Ok(())
    }

    pub async fn gsb<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> anyhow::Result<()> {
        let market = MARKET.get_or_init_market(&ctx.component())?;
        Ok(market.bind_gsb(BUS_ID, private::BUS_ID).await?)
    }

    pub fn rest<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> actix_web::Scope {
        match MARKET.get_or_init_market(&ctx.component()) {
            Ok(market) => MarketService::bind_rest(market),
            Err(e) => {
                log::error!("REST API initialization failed: {}", e);
                panic!("Market Service initialization impossible: {}", e)
            }
        }
    }

    pub fn bind_rest(myself: Arc<MarketService>) -> actix_web::Scope {
        actix_web::web::scope(ya_client::model::market::MARKET_API_PATH)
            .data(myself)
            .app_data(rest_api::path_config())
            .extend(rest_api::provider::register_endpoints)
            .extend(rest_api::requestor::register_endpoints)
    }

    pub async fn get_offers(&self, id: Option<Identity>) -> Result<Vec<Offer>, MarketError> {
        Ok(self.matcher.store.get_offers(id).await?)
    }

    pub async fn subscribe_offer(
        &self,
        offer: &Offer,
        id: &Identity,
    ) -> Result<SubscriptionId, MarketError> {
        let offer = self.matcher.subscribe_offer(offer, id).await?;
        self.provider_engine.subscribe_offer(&offer).await?;
        Ok(offer.id)
    }

    pub async fn unsubscribe_offer(
        &self,
        offer_id: &SubscriptionId,
        id: &Identity,
    ) -> Result<(), MarketError> {
        // TODO: Authorize unsubscribe caller.
        self.provider_engine.unsubscribe_offer(offer_id).await?;
        Ok(self.matcher.unsubscribe_offer(offer_id, id).await?)
    }

    pub async fn subscribe_demand(
        &self,
        demand: &Demand,
        id: &Identity,
    ) -> Result<SubscriptionId, MarketError> {
        let demand = self.matcher.subscribe_demand(demand, id).await?;
        self.requestor_engine.subscribe_demand(&demand).await?;
        Ok(demand.id)
    }

    pub async fn unsubscribe_demand(
        &self,
        demand_id: &SubscriptionId,
        id: &Identity,
    ) -> Result<(), MarketError> {
        // TODO: Authorize unsubscribe caller.

        self.requestor_engine.unsubscribe_demand(demand_id).await?;
        // TODO: shouldn't remove precede negotiation unsubscribe?
        Ok(self.matcher.unsubscribe_demand(demand_id, id).await?)
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
