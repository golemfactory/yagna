use actix_web::web::Data;
use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use metrics::counter;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::config::Config;
use crate::db::dao::AgreementDao;
use crate::db::model::{AgreementId, AppSessionId, Owner, SubscriptionId};
use crate::identity::{IdentityApi, IdentityGSB};
use crate::matcher::error::{
    DemandError, MatcherError, MatcherInitError, QueryDemandsError, QueryOfferError,
    QueryOffersError,
};
use crate::matcher::{store::SubscriptionStore, Matcher};
use crate::negotiation::error::{
    AgreementError, AgreementEventsError, NegotiationError, NegotiationInitError,
};
use crate::negotiation::{EventNotifier, ProviderBroker, RequestorBroker};
use crate::rest_api;
use crate::testing::AgreementState;

use ya_client::model::market::{
    Agreement, AgreementListEntry, AgreementOperationEvent as ClientAgreementEvent, Demand,
    NewDemand, NewOffer, Offer, Reason, Role,
};
use ya_core_model::market::{local, BUS_ID};
use ya_service_api_interfaces::{Provider, Service};
use ya_service_api_web::middleware::Identity;

use crate::db::DbMixedExecutor;
use ya_service_api_web::scope::ExtendableScope;

pub mod agreement;

#[derive(Error, Debug)]
pub enum MarketError {
    #[error(transparent)]
    Matcher(#[from] MatcherError),
    #[error(transparent)]
    QueryDemandsError(#[from] QueryDemandsError),
    #[error(transparent)]
    QueryOfferError(#[from] QueryOfferError),
    #[error(transparent)]
    QueryOffersError(#[from] QueryOffersError),
    #[error(transparent)]
    DemandError(#[from] DemandError),
    #[error(transparent)]
    Negotiation(#[from] NegotiationError),
}

#[derive(Error, Debug)]
pub enum MarketInitError {
    #[error("Failed to initialize Matcher. Error: {0}.")]
    Matcher(#[from] MatcherInitError),
    #[error("Failed to initialize negotiation engine. Error: {0}.")]
    Negotiation(#[from] NegotiationInitError),
    #[error("Failed to migrate market database. Error: {0}.")]
    Migration(#[from] anyhow::Error),
    #[error("Failed to initialize config. Error: {0}.")]
    Config(#[from] structopt::clap::Error),
    #[error("Failed to initialize in memory market database. Error: {0}.")]
    InMemory(anyhow::Error),
}

/// Structure connecting all market objects.
pub struct MarketService {
    pub db: DbMixedExecutor,
    pub matcher: Matcher,
    pub provider_engine: ProviderBroker,
    pub requestor_engine: RequestorBroker,
}

impl MarketService {
    pub fn new(
        db: &DbMixedExecutor,
        identity_api: Arc<dyn IdentityApi>,
        config: Arc<Config>,
    ) -> Result<Self, MarketInitError> {
        counter!("market.offers.subscribed", 0);
        counter!("market.offers.unsubscribed", 0);
        counter!("market.offers.expired", 0);
        counter!("market.demands.subscribed", 0);
        counter!("market.demands.unsubscribed", 0);
        counter!("market.demands.expired", 0);

        db.ram_db
            .apply_migration(crate::db::migrations::run_with_output)?;
        db.disk_db
            .apply_migration(crate::db::migrations::run_with_output)?;

        let store = SubscriptionStore::new(db.clone(), config.clone());
        let (matcher, listeners) = Matcher::new(store.clone(), identity_api, config.clone())?;

        // We need the same notifier for both Provider and Requestor implementation since we have
        // single endpoint and both implementations are able to add events.
        let agreement_notifier = EventNotifier::<AppSessionId>::new();

        let provider_engine = ProviderBroker::new(
            db.clone(),
            store.clone(),
            agreement_notifier.clone(),
            config.clone(),
        )?;
        let requestor_engine = RequestorBroker::new(
            db.clone(),
            store.clone(),
            listeners.proposal_receiver,
            agreement_notifier,
            config.clone(),
        )?;
        let cleaner_db = db.clone();
        tokio::spawn(async move {
            crate::db::dao::cleaner::clean_forever(cleaner_db, config.db.clone()).await;
        });

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
        local_prefix: &str,
    ) -> Result<(), MarketInitError> {
        self.matcher.bind_gsb(public_prefix, local_prefix).await?;
        self.provider_engine
            .bind_gsb(public_prefix, local_prefix)
            .await?;
        self.requestor_engine
            .bind_gsb(public_prefix, local_prefix)
            .await?;
        agreement::bind_gsb(self.db.clone(), public_prefix, local_prefix).await;
        Ok(())
    }

    pub async fn gsb<Context: Provider<Self, DbMixedExecutor>>(
        ctx: &Context,
    ) -> anyhow::Result<()> {
        let market = MARKET.get_or_init_market(&ctx.component())?;
        Ok(market.bind_gsb(BUS_ID, local::BUS_ID).await?)
    }

    pub fn rest<Context: Provider<Self, DbMixedExecutor>>(ctx: &Context) -> actix_web::Scope {
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
            .app_data(Data::new(myself))
            .app_data(Data::new(rest_api::path_config()))
            .app_data(Data::new(rest_api::json_config()))
            .extend(rest_api::common::register_endpoints)
            .extend(rest_api::provider::register_endpoints)
            .extend(rest_api::requestor::register_endpoints)
    }

    // TODO: (re)move this
    pub async fn get_offers(&self, id: Option<Identity>) -> Result<Vec<Offer>, MarketError> {
        Ok(self
            .matcher
            .store
            .get_client_offers(id.map(|identity| identity.identity))
            .await?)
    }

    pub async fn get_demands(&self, id: Option<Identity>) -> Result<Vec<Demand>, MarketError> {
        Ok(self
            .matcher
            .store
            .get_client_demands(id.map(|identitty| identitty.identity))
            .await?)
    }

    pub async fn subscribe_offer(
        &self,
        offer: &NewOffer,
        id: &Identity,
    ) -> Result<SubscriptionId, MarketError> {
        let offer = self.matcher.subscribe_offer(offer, id).await?;
        self.provider_engine.subscribe_offer(&offer).await?;

        counter!("market.offers.subscribed", 1);
        Ok(offer.id)
    }

    pub async fn unsubscribe_offer(
        &self,
        offer_id: &SubscriptionId,
        id: &Identity,
    ) -> Result<(), MarketError> {
        // TODO: Authorize unsubscribe caller.
        self.provider_engine.unsubscribe_offer(offer_id).await?;
        self.matcher.unsubscribe_offer(offer_id, id).await?;

        counter!("market.offers.unsubscribed", 1);
        Ok(())
    }

    pub async fn subscribe_demand(
        &self,
        demand: &NewDemand,
        id: &Identity,
    ) -> Result<SubscriptionId, MarketError> {
        let demand = self.matcher.subscribe_demand(demand, id).await?;
        self.requestor_engine.subscribe_demand(&demand).await?;

        counter!("market.demands.subscribed", 1);
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
        self.matcher.unsubscribe_demand(demand_id, id).await?;

        counter!("market.demands.unsubscribed", 1);
        Ok(())
    }

    pub async fn list_agreements(
        &self,
        id: &Identity,
        state: Option<AgreementState>,
        before: Option<DateTime<Utc>>,
        after: Option<DateTime<Utc>>,
        app_sesssion_id: Option<String>,
    ) -> Result<Vec<AgreementListEntry>, AgreementError> {
        let agreements = self
            .db
            .as_dao::<AgreementDao>()
            .list(Some(id.identity), state, before, after, app_sesssion_id)
            .await
            .map_err(|e| AgreementError::Internal(e.to_string()))?;

        let mut result = Vec::new();
        let naive_to_utc = |ts| DateTime::<Utc>::from_utc(ts, Utc);

        for agreement in agreements {
            let role = match agreement.id.owner() {
                Owner::Provider => Role::Provider,
                Owner::Requestor => Role::Requestor,
            };

            result.push(AgreementListEntry {
                id: agreement.id.into_client(),
                creation_ts: naive_to_utc(agreement.creation_ts),
                approve_ts: agreement.approved_ts.map(naive_to_utc),
                role,
            });
        }

        Ok(result)
    }

    pub async fn get_agreement(
        &self,
        agreement_id: &AgreementId,
        id: &Identity,
    ) -> Result<Agreement, AgreementError> {
        match self
            .db
            .as_dao::<AgreementDao>()
            .select(agreement_id, Some(id.identity), Utc::now().naive_utc())
            .await
            .map_err(|e| AgreementError::Get(agreement_id.to_string(), e))?
        {
            Some(agreement) => Ok(agreement
                .into_client()
                .map_err(|e| AgreementError::Internal(e.to_string()))?),
            None => Err(AgreementError::NotFound(agreement_id.to_string())),
        }
    }

    pub async fn query_agreement_events(
        &self,
        session_id: &AppSessionId,
        timeout: f32,
        max_events: Option<i32>,
        after_timestamp: DateTime<Utc>,
        id: &Identity,
    ) -> Result<Vec<ClientAgreementEvent>, AgreementEventsError> {
        Ok(self
            .requestor_engine
            .common
            .query_agreement_events(session_id, timeout, max_events, after_timestamp, id)
            .await?
            .into_iter()
            .map(|event| event.into_client())
            .collect())
    }

    pub async fn terminate_agreement(
        &self,
        id: Identity,
        client_agreement_id: String,
        reason: Option<Reason>,
    ) -> Result<(), AgreementError> {
        self.requestor_engine
            .common
            .terminate_agreement(id, client_agreement_id, reason)
            .await
    }
}

impl Service for MarketService {
    type Cli = crate::cli::Command;
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
        db: &DbMixedExecutor,
    ) -> Result<Arc<MarketService>, MarketInitError> {
        let mut guarded_market = self.locked_market.lock().unwrap();
        if let Some(market) = &*guarded_market {
            Ok(market.clone())
        } else {
            let identity_api = IdentityGSB::new();
            let config = Arc::new(Config::from_env()?);
            let market = Arc::new(MarketService::new(db, identity_api, config)?);
            *guarded_market = Some(market.clone());
            Ok(market)
        }
    }
}

lazy_static! {
    static ref MARKET: StaticMarket = StaticMarket::new();
}
