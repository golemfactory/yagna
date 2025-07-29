pub mod legacy;

use actix_web::web::{Data, Json, Path as WebPath, Query};
use actix_web::{HttpResponse, Responder, Scope};
use chrono::{Duration, Utc};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use ya_client_model::market::MARKET_API_PATH;

use ya_agreement_utils::agreement::expand;
use ya_agreement_utils::{OfferTemplate, ProposalView};
use ya_client::model::market::agreement;
use ya_client::model::market::proposal;
use ya_client::model::market::AgreementOperationEvent;
use ya_client::model::market::{Agreement, AgreementListEntry, Demand, Offer, Role};
use ya_client::model::market::{NewDemand, NewOffer, NewProposal, Reason};
use ya_client::model::ErrorMessage;
use ya_client::model::NodeId;
use ya_core_model::market;
use ya_market::testing::{
    AgreementId, Demand as MarketDemand, Offer as MarketOffer, Owner, PathSubscription, ProposalId,
    QueryAgreementEvents, QueryTimeoutMaxEvents, SubscriptionId,
};
use ya_service_api_web::middleware::Identity;
use ya_service_api_web::scope::ExtendableScope;
use ya_service_bus::typed as bus;

/// Wrapper struct for Offer that can be extended with control elements for mocking market
#[derive(Clone, Debug)]
pub struct OfferEntry {
    pub offer: MarketOffer,
    // TODO: Add control elements here for mocking market behavior
    // pub control_elements: MarketControlElements,
}

/// Wrapper struct for Demand that can be extended with control elements for mocking market
#[derive(Clone, Debug)]
pub struct DemandEntry {
    pub demand: MarketDemand,
    // TODO: Add control elements here for mocking market behavior
    // pub control_elements: MarketControlElements,
}

/// Market that doesn't wrap real Market module, but simulates it's
/// behavior by providing GSB bindings for crucial messages.
#[derive(Clone)]
pub struct FakeMarket {
    name: String,
    _testdir: PathBuf,

    inner: Arc<RwLock<FakeMarketInner>>,
}

pub struct FakeMarketInner {
    agreements: HashMap<AgreementId, Agreement>,
    offer_subscriptions: HashMap<SubscriptionId, OfferEntry>,
    demand_subscriptions: HashMap<SubscriptionId, DemandEntry>,
}

impl FakeMarket {
    pub fn new(name: &str, testdir: &Path) -> Self {
        FakeMarket {
            name: name.to_string(),
            _testdir: testdir.to_path_buf(),
            inner: Arc::new(RwLock::new(FakeMarketInner {
                agreements: HashMap::new(),
                offer_subscriptions: HashMap::new(),
                demand_subscriptions: HashMap::new(),
            })),
        }
    }

    pub fn bind_rest(&self) -> Scope {
        actix_web::web::scope(MARKET_API_PATH)
            .app_data(Data::new(self.clone()))
            .extend(register_common_endpoints)
            .extend(register_provider_endpoints)
            .extend(register_requestor_endpoints)
    }

    pub async fn bind_gsb(&self) -> anyhow::Result<()> {
        log::info!("FakeMarket ({}) - binding GSB", self.name);

        let self_ = self.clone();
        bus::bind_with_caller(
            market::local::BUS_ID,
            move |sender: String, msg: market::GetAgreement| {
                let self_ = self_.clone();
                async move { self_.get_agreement_handler(sender, msg).await }
            },
        );
        let self_ = self.clone();
        bus::bind_with_caller(
            market::local::BUS_ID,
            move |sender: String, msg: market::ListAgreements| {
                let self_ = self_.clone();
                async move { self_.list_agreements_handler(sender, msg).await }
            },
        );
        Ok(())
    }

    async fn get_agreement_handler(
        &self,
        _sender_id: String,
        msg: market::GetAgreement,
    ) -> Result<Agreement, market::RpcMessageError> {
        let owner = match msg.role {
            Role::Provider => Owner::Provider,
            Role::Requestor => Owner::Requestor,
        };

        let agreement_id = AgreementId::from_client(&msg.agreement_id, owner)
            .map_err(|e| market::RpcMessageError::Market(e.to_string()))?;

        self.get_agreement(agreement_id.clone())
            .await
            .ok_or_else(|| {
                market::RpcMessageError::NotFound(format!("Agreement id: {agreement_id}"))
            })
    }

    async fn list_agreements_handler(
        &self,
        _sender_id: String,
        msg: market::ListAgreements,
    ) -> Result<Vec<market::AgreementListEntry>, market::RpcMessageError> {
        let lock = self.inner.read().await;
        let agreements = lock
            .agreements
            .iter()
            .filter(|(_, agreement)| {
                msg.app_session_id.is_none() || agreement.app_session_id == msg.app_session_id
            })
            .filter(|(_, agreement)| msg.state.is_none() || agreement.state == msg.state.unwrap())
            .filter(|(_, agreement)| {
                msg.before_date.is_none() || agreement.timestamp < msg.before_date.unwrap()
            })
            .filter(|(_, agreement)| {
                msg.after_date.is_none() || agreement.timestamp > msg.after_date.unwrap()
            })
            .map(|(id, agreement)| AgreementListEntry {
                id: agreement.agreement_id.clone(),
                timestamp: agreement.timestamp,
                approved_date: agreement.approved_date,
                role: match id.owner() {
                    Owner::Provider => Role::Provider,
                    Owner::Requestor => Role::Requestor,
                },
            })
            .collect();

        Ok(agreements)
    }

    pub async fn get_agreement(&self, agreement_id: AgreementId) -> Option<Agreement> {
        self.inner
            .read()
            .await
            .agreements
            .get(&agreement_id)
            .cloned()
    }

    pub async fn add_agreement(&self, agreement: Agreement) {
        let provider_id =
            AgreementId::from_client(&agreement.agreement_id, Owner::Provider).unwrap();
        let requestor_id =
            AgreementId::from_client(&agreement.agreement_id, Owner::Requestor).unwrap();

        let mut lock = self.inner.write().await;
        lock.agreements.insert(provider_id, agreement.clone());
        lock.agreements.insert(requestor_id, agreement);
    }

    pub async fn add_offer_subscription(&self, offer_entry: OfferEntry) {
        let mut lock = self.inner.write().await;
        lock.offer_subscriptions
            .insert(offer_entry.offer.id.clone(), offer_entry);
    }

    pub async fn add_demand_subscription(&self, demand_entry: DemandEntry) {
        let mut lock = self.inner.write().await;
        lock.demand_subscriptions
            .insert(demand_entry.demand.id.clone(), demand_entry);
    }

    pub async fn get_offer_subscription(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Option<OfferEntry> {
        let lock = self.inner.read().await;
        lock.offer_subscriptions.get(subscription_id).cloned()
    }

    pub async fn get_demand_subscription(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Option<DemandEntry> {
        let lock = self.inner.read().await;
        lock.demand_subscriptions.get(subscription_id).cloned()
    }

    pub async fn remove_offer_subscription(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Option<OfferEntry> {
        let mut lock = self.inner.write().await;
        lock.offer_subscriptions.remove(subscription_id)
    }

    pub async fn remove_demand_subscription(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Option<DemandEntry> {
        let mut lock = self.inner.write().await;
        lock.demand_subscriptions.remove(subscription_id)
    }

    pub async fn list_offer_subscriptions(&self) -> Vec<OfferEntry> {
        let lock = self.inner.read().await;
        lock.offer_subscriptions.values().cloned().collect()
    }

    pub async fn list_demand_subscriptions(&self) -> Vec<DemandEntry> {
        let lock = self.inner.read().await;
        lock.demand_subscriptions.values().cloned().collect()
    }

    pub fn create_fake_agreement(
        requestor_id: NodeId,
        provider_id: NodeId,
    ) -> anyhow::Result<Agreement> {
        let offer = Self::create_default_offer(provider_id)?;
        let demand = Self::create_default_demand(requestor_id)?;

        Self::agreement_from(offer, demand)
    }

    pub fn agreement_from(offer: ProposalView, demand: ProposalView) -> anyhow::Result<Agreement> {
        let timestamp = Utc::now();
        let agreement_id = ProposalId::generate_id(
            &SubscriptionId::from_str(&offer.id)?,
            &SubscriptionId::from_str(&demand.id)?,
            &timestamp.naive_utc(),
            Owner::Requestor,
        );
        Ok(Agreement {
            agreement_id: agreement_id.into_client(),
            demand: Demand {
                properties: demand.content.properties,
                constraints: demand.content.constraints,
                demand_id: demand.id,
                requestor_id: demand.issuer,
                timestamp: demand.timestamp,
                expiration: demand.timestamp + Duration::hours(1),
            },
            offer: Offer {
                properties: offer.content.properties,
                constraints: offer.content.constraints,
                offer_id: offer.id,
                provider_id: offer.issuer,
                timestamp: offer.timestamp,
                expiration: offer.timestamp + Duration::hours(1),
            },
            valid_to: timestamp + Duration::hours(2),
            approved_date: None,
            state: agreement::State::Approved,
            timestamp,
            app_session_id: None,
            proposed_signature: None,
            approved_signature: None,
            committed_signature: None,
        })
    }

    pub fn create_default_offer(provider_id: NodeId) -> anyhow::Result<ProposalView> {
        let template = OfferTemplate {
            properties: expand(serde_json::from_str(r#"{ "any": "thing" }"#).unwrap()),
            constraints: "()".to_string(),
        };
        Self::create_demand(provider_id, template)
    }

    pub fn create_offer(
        provider_id: NodeId,
        content: OfferTemplate,
    ) -> anyhow::Result<ProposalView> {
        let offer = ProposalView {
            id: "".to_string(),
            content: content.flatten(),
            issuer: provider_id,
            state: proposal::State::Accepted,
            timestamp: Utc::now(),
        };

        let id = subscription_id_from(&offer)?.to_string();
        Ok(ProposalView { id, ..offer })
    }

    pub fn create_default_demand(requestor_id: NodeId) -> anyhow::Result<ProposalView> {
        let basic_props = json!({
            "golem.com.payment.platform.erc20-holesky-tglm.address": requestor_id.to_string(),
            "golem.com.payment.protocol.version": 3,
            "golem.com.payment.chosen-platform": "erc20-holesky-tglm",
        });

        let template = OfferTemplate {
            properties: expand(basic_props),
            constraints: "()".to_string(),
        };
        Self::create_demand(requestor_id, template)
    }
    pub fn create_demand(
        requestor_id: NodeId,
        content: OfferTemplate,
    ) -> anyhow::Result<ProposalView> {
        let demand = ProposalView {
            id: "".to_string(),
            content: content.flatten(),
            issuer: requestor_id,
            state: proposal::State::Accepted,
            timestamp: Utc::now(),
        };

        let id = subscription_id_from(&demand)?.to_string();
        Ok(ProposalView { id, ..demand })
    }
}

fn register_common_endpoints(scope: Scope) -> Scope {
    scope
        .service(list_agreements)
        .service(collect_agreement_events)
        .service(get_agreement)
        .service(terminate_agreement)
        .service(get_agreement_terminate_reason)
        .service(scan_begin)
        .service(scan_collect)
        .service(scan_end)
}

fn register_provider_endpoints(scope: Scope) -> Scope {
    scope
        .service(subscribe_offer)
        .service(get_offers)
        .service(unsubscribe_offer)
        .service(collect_offer_events)
        .service(counter_proposal_offer)
        .service(get_proposal_offer)
        .service(reject_proposal_offer)
        .service(approve_agreement)
        .service(reject_agreement)
}

fn register_requestor_endpoints(scope: Scope) -> Scope {
    scope
        .service(subscribe_demand)
        .service(get_demands)
        .service(unsubscribe_demand)
        .service(collect_demand_events)
        .service(counter_proposal_demand)
        .service(get_proposal_demand)
        .service(reject_proposal_demand)
        .service(create_agreement)
        .service(confirm_agreement)
        .service(wait_for_approval)
        .service(cancel_agreement)
}

// Common endpoints
#[actix_web::get("/agreements")]
async fn list_agreements(market: Data<FakeMarket>, _query: Query<()>) -> impl Responder {
    let lock = market.inner.read().await;
    let agreements: Vec<AgreementListEntry> = lock
        .agreements
        .iter()
        .map(|(id, agreement)| AgreementListEntry {
            id: agreement.agreement_id.clone(),
            timestamp: agreement.timestamp,
            approved_date: agreement.approved_date,
            role: match id.owner() {
                Owner::Provider => Role::Provider,
                Owner::Requestor => Role::Requestor,
            },
        })
        .collect();
    HttpResponse::Ok().json(agreements)
}

#[actix_web::get("/agreementEvents")]
async fn collect_agreement_events(
    _market: Data<FakeMarket>,
    query: Query<QueryAgreementEvents>,
) -> impl Responder {
    let timeout = std::time::Duration::from_secs_f32(query.into_inner().timeout);
    tokio::time::sleep(timeout).await;
    HttpResponse::Ok().json(Vec::<AgreementOperationEvent>::new())
}

#[actix_web::get("/agreements/{agreement_id}")]
async fn get_agreement(market: Data<FakeMarket>, path: WebPath<String>) -> impl Responder {
    let agreement_id = path.into_inner();
    let lock = market.inner.read().await;

    // Try to find agreement by ID
    for (_, agreement) in &lock.agreements {
        if agreement.agreement_id == agreement_id {
            return HttpResponse::Ok().json(agreement);
        }
    }

    HttpResponse::NotFound().json(ErrorMessage::new("Agreement not found"))
}

#[actix_web::post("/agreements/{agreement_id}/terminate")]
async fn terminate_agreement(
    _market: Data<FakeMarket>,
    _path: WebPath<String>,
    _body: Json<Option<Reason>>,
) -> impl Responder {
    HttpResponse::Ok().finish()
}

#[actix_web::get("/agreements/{agreement_id}/terminate/reason")]
async fn get_agreement_terminate_reason(
    _market: Data<FakeMarket>,
    _path: WebPath<String>,
) -> impl Responder {
    HttpResponse::Ok().json(Reason::new("Mock termination reason"))
}

#[actix_web::post("/scan")]
async fn scan_begin(_market: Data<FakeMarket>, _body: Json<serde_json::Value>) -> impl Responder {
    HttpResponse::Created().json("mock-scan-id")
}

#[actix_web::get("/scan/{scan_id}/events")]
async fn scan_collect(
    _market: Data<FakeMarket>,
    _path: WebPath<String>,
    _query: Query<()>,
) -> impl Responder {
    HttpResponse::Ok().json(Vec::<Offer>::new())
}

#[actix_web::delete("/scan/{scan_id}")]
async fn scan_end(_market: Data<FakeMarket>, _path: WebPath<String>) -> impl Responder {
    HttpResponse::NoContent().finish()
}

// Provider endpoints
#[actix_web::post("/offers")]
async fn subscribe_offer(
    market: Data<FakeMarket>,
    body: Json<NewOffer>,
    id: Identity,
) -> impl Responder {
    let new_offer = body.into_inner();
    let creation_ts = Utc::now().naive_utc();
    let expiration_ts = creation_ts + Duration::hours(1);

    let market_offer =
        match MarketOffer::from_new(&new_offer, id.identity, creation_ts, expiration_ts) {
            Ok(offer) => offer,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(ErrorMessage::new(format!("Unable to create offer: {e}")))
            }
        };

    let id = market_offer.id.to_string();
    market
        .add_offer_subscription(OfferEntry {
            offer: market_offer,
        })
        .await;

    HttpResponse::Created().json(id)
}

#[actix_web::get("/offers")]
async fn get_offers(market: Data<FakeMarket>, id: Identity) -> impl Responder {
    let offer_entries = market.list_offer_subscriptions().await;
    let offers: Vec<Offer> = offer_entries
        .into_iter()
        .filter(|entry| entry.offer.node_id == id.identity)
        .filter_map(|entry| entry.offer.into_client_offer().ok())
        .collect();
    HttpResponse::Ok().json(offers)
}

#[actix_web::delete("/offers/{subscription_id}")]
async fn unsubscribe_offer(
    market: Data<FakeMarket>,
    path: WebPath<String>,
    _id: Identity,
) -> impl Responder {
    let subscription_id_str = path.into_inner();
    if let Ok(subscription_id) = SubscriptionId::from_str(&subscription_id_str) {
        if let Some(_) = market.remove_offer_subscription(&subscription_id).await {
            return HttpResponse::NoContent().finish();
        }
    }
    HttpResponse::NotFound().finish()
}

#[actix_web::get("/offers/{subscription_id}/events")]
async fn collect_offer_events(
    _market: Data<FakeMarket>,
    _path: WebPath<PathSubscription>,
    query: Query<QueryTimeoutMaxEvents>,
) -> impl Responder {
    let timeout = std::time::Duration::from_secs_f32(query.into_inner().timeout);
    tokio::time::sleep(timeout).await;
    HttpResponse::Ok().json(Vec::<serde_json::Value>::new())
}

#[actix_web::post("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn counter_proposal_offer(
    _market: Data<FakeMarket>,
    _path: WebPath<(String, String)>,
    _body: Json<NewProposal>,
) -> impl Responder {
    HttpResponse::Ok().json("mock-proposal-id")
}

#[actix_web::get("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn get_proposal_offer(
    _market: Data<FakeMarket>,
    _path: WebPath<(String, String)>,
) -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "proposalId": "mock-proposal-id",
        "properties": {},
        "constraints": "()"
    }))
}

#[actix_web::post("/offers/{subscription_id}/proposals/{proposal_id}/reject")]
async fn reject_proposal_offer(
    _market: Data<FakeMarket>,
    _path: WebPath<(String, String)>,
    _body: Json<Option<Reason>>,
) -> impl Responder {
    HttpResponse::NoContent().finish()
}

#[actix_web::post("/agreements/{agreement_id}/approve")]
async fn approve_agreement(
    _market: Data<FakeMarket>,
    _path: WebPath<String>,
    _query: Query<()>,
) -> impl Responder {
    HttpResponse::NoContent().finish()
}

#[actix_web::post("/agreements/{agreement_id}/reject")]
async fn reject_agreement(
    _market: Data<FakeMarket>,
    _path: WebPath<String>,
    _body: Json<Option<Reason>>,
) -> impl Responder {
    HttpResponse::Ok().finish()
}

// Requestor endpoints
#[actix_web::post("/demands")]
async fn subscribe_demand(
    market: Data<FakeMarket>,
    body: Json<NewDemand>,
    id: Identity,
) -> impl Responder {
    let new_demand = body.into_inner();
    let creation_ts = Utc::now().naive_utc();
    let expiration_ts = creation_ts + Duration::hours(1);

    let market_demand = match MarketDemand::from_new(&new_demand, &id, creation_ts, expiration_ts) {
        Ok(demand) => demand,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(ErrorMessage::new(format!("Unable to create demand: {e}")))
        }
    };

    let id = market_demand.id.to_string();
    market
        .add_demand_subscription(DemandEntry {
            demand: market_demand,
        })
        .await;

    HttpResponse::Created().json(id)
}

#[actix_web::get("/demands")]
async fn get_demands(market: Data<FakeMarket>, id: Identity) -> impl Responder {
    let demand_entries = market.list_demand_subscriptions().await;
    let demands: Vec<Demand> = demand_entries
        .into_iter()
        .filter(|entry| entry.demand.node_id == id.identity)
        .filter_map(|entry| entry.demand.into_client_demand().ok())
        .collect();
    HttpResponse::Ok().json(demands)
}

#[actix_web::delete("/demands/{subscription_id}")]
async fn unsubscribe_demand(
    market: Data<FakeMarket>,
    path: WebPath<String>,
    _id: Identity,
) -> impl Responder {
    let subscription_id_str = path.into_inner();
    if let Ok(subscription_id) = SubscriptionId::from_str(&subscription_id_str) {
        if let Some(_) = market.remove_demand_subscription(&subscription_id).await {
            return HttpResponse::NoContent().finish();
        }
    }
    HttpResponse::NotFound().finish()
}

#[actix_web::get("/demands/{subscription_id}/events")]
async fn collect_demand_events(
    _market: Data<FakeMarket>,
    _path: WebPath<String>,
    query: Query<QueryTimeoutMaxEvents>,
) -> impl Responder {
    let timeout = std::time::Duration::from_secs_f32(query.into_inner().timeout);
    tokio::time::sleep(timeout).await;
    HttpResponse::Ok().json(Vec::<serde_json::Value>::new())
}

#[actix_web::post("/demands/{subscription_id}/proposals/{proposal_id}")]
async fn counter_proposal_demand(
    _market: Data<FakeMarket>,
    _path: WebPath<(String, String)>,
    _body: Json<NewProposal>,
) -> impl Responder {
    HttpResponse::Ok().json("mock-proposal-id")
}

#[actix_web::get("/demands/{subscription_id}/proposals/{proposal_id}")]
async fn get_proposal_demand(
    _market: Data<FakeMarket>,
    _path: WebPath<(String, String)>,
) -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "proposalId": "mock-proposal-id",
        "properties": {},
        "constraints": "()"
    }))
}

#[actix_web::post("/demands/{subscription_id}/proposals/{proposal_id}/reject")]
async fn reject_proposal_demand(
    _market: Data<FakeMarket>,
    _path: WebPath<(String, String)>,
    _body: Json<Option<Reason>>,
) -> impl Responder {
    HttpResponse::NoContent().finish()
}

#[actix_web::post("/agreements")]
async fn create_agreement(
    _market: Data<FakeMarket>,
    _body: Json<serde_json::Value>,
) -> impl Responder {
    HttpResponse::Ok().json("mock-agreement-id")
}

#[actix_web::post("/agreements/{agreement_id}/confirm")]
async fn confirm_agreement(
    _market: Data<FakeMarket>,
    _path: WebPath<String>,
    _query: Query<()>,
) -> impl Responder {
    HttpResponse::NoContent().finish()
}

#[actix_web::post("/agreements/{agreement_id}/wait")]
async fn wait_for_approval(
    _market: Data<FakeMarket>,
    _path: WebPath<String>,
    _query: Query<()>,
) -> impl Responder {
    HttpResponse::NoContent().finish()
}

#[actix_web::post("/agreements/{agreement_id}/cancel")]
async fn cancel_agreement(
    _market: Data<FakeMarket>,
    _path: WebPath<String>,
    _body: Json<Option<Reason>>,
) -> impl Responder {
    HttpResponse::Ok().finish()
}

fn subscription_id_from(template: &ProposalView) -> anyhow::Result<SubscriptionId> {
    let id = SubscriptionId::generate_id(
        &serde_json::to_string_pretty(&template.content.properties)?,
        &serde_json::to_string_pretty(&template.content.constraints)?,
        &template.issuer,
        &template.timestamp.naive_utc(),
        &(template.timestamp + Duration::hours(2)).naive_utc(),
    );
    Ok(id)
}
