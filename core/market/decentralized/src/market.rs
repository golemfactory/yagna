use lazy_static::lazy_static;
use std::env::current_dir;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::matcher::{Matcher, MatcherInitError};
use crate::negotiation::NegotiationInitError;
use crate::negotiation::{ProviderNegotiationEngine, RequestorNegotiationEngine};
use crate::protocol::{DiscoveryBuilder, DiscoveryGSB};

use ya_core_model::market::BUS_ID;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};

#[derive(Error, Debug)]
pub enum MarketError {}

#[derive(Error, Debug)]
pub enum MarketInitError {
    #[error("Failed to initialize Offers matcher. Error: {}.", .0)]
    Matcher(#[from] MatcherInitError),
    #[error("Failed to initialize negotiation engine. Error: {}.", .0)]
    Negotiation(#[from] NegotiationInitError),
}

/// Structure connecting all market objects.
pub struct MarketService {
    matcher: Arc<Matcher>,
    provider_negotiation_engine: Arc<ProviderNegotiationEngine>,
    requestor_negotiation_engine: Arc<RequestorNegotiationEngine>,
}

impl MarketService {
    pub fn new(db: &DbExecutor) -> Result<Self, MarketInitError> {
        // TODO: Set Matcher independent parameters here or remove this todo.
        let builder = DiscoveryBuilder::new();

        let (matcher, listeners) = Matcher::new::<DiscoveryGSB>(builder, db)?;
        let provider_engine = ProviderNegotiationEngine::new(db.clone())?;
        let requestor_engine =
            RequestorNegotiationEngine::new(db.clone(), listeners.proposal_receiver)?;

        Ok(MarketService {
            matcher: Arc::new(matcher),
            provider_negotiation_engine: provider_engine,
            requestor_negotiation_engine: requestor_engine,
        })
    }

    pub async fn bind_gsb(&self, prefix: String) -> Result<(), MarketInitError> {
        self.matcher.bind_gsb(prefix.clone()).await?;
        self.provider_negotiation_engine
            .bind_gsb(prefix.clone())
            .await?;
        self.requestor_negotiation_engine.bind_gsb(prefix).await?;
        Ok(())
    }

    pub async fn gsb<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> anyhow::Result<()> {
        let market = MARKET.get_or_init_market(&ctx.component())?;
        Ok(market.bind_gsb(BUS_ID.to_string()).await?)
    }

    pub fn rest<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> actix_web::Scope {
        let market = match MARKET.get_or_init_market(&ctx.component()) {
            Ok(market) => market,
            Err(error) => {
                log::error!("{}", error);
                panic!("Market initialization impossible. Check error logs.")
            }
        };
        actix_web::web::scope(crate::MARKET_API_PATH).data(market)
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
