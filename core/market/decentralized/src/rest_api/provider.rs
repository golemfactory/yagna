use actix_web::web::{Data, Json, Path, Query};
use actix_web::{HttpResponse, Responder, Scope};
use std::sync::Arc;

use super::{
    PathAgreement, PathSubscription, PathSubscriptionProposal, QueryTimeout, QueryTimeoutMaxEvents,
};
use crate::market::MarketService;

use ya_client::model::market::{Offer, Proposal};
use ya_service_api_web::middleware::Identity;

// This file contains market REST endpoints. Responsibility of these functions
// is calling respective functions in market modules and mapping return values
// to http responses. No market logic is allowed here.

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .service(subscribe)
        .service(get_offers)
        .service(unsubscribe)
        .service(collect)
        .service(counter_proposal)
        .service(get_proposal)
        .service(reject_proposal)
        .service(approve_agreement)
        .service(reject_agreement)
        .service(terminate_agreement)
        .service(get_agreement)
}

#[actix_web::post("/offers")]
async fn subscribe(
    market: Data<Arc<MarketService>>,
    body: Json<Offer>,
    id: Identity,
) -> impl Responder {
    market
        .subscribe_offer(&body.into_inner(), &id)
        .await
        .map(|id| HttpResponse::Created().json(id))
}

#[actix_web::get("/offers")]
async fn get_offers(market: Data<Arc<MarketService>>, id: Identity) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::delete("/offers/{subscription_id}")]
async fn unsubscribe(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscription>,
    id: Identity,
) -> impl Responder {
    market
        .unsubscribe_offer(&path.into_inner().subscription_id, &id)
        .await
        .map(|x| HttpResponse::Ok().json("Ok"))
}

#[actix_web::get("/offers/{subscription_id}/events")]
async fn collect(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscription>,
    query: Query<QueryTimeoutMaxEvents>,
    id: Identity,
) -> impl Responder {
    let subscription_id = path.into_inner().subscription_id;
    let timeout = query.timeout;
    let max_events = query.max_events;
    market
        .provider_engine
        .query_events(&subscription_id, timeout, max_events)
        .await
        .map(|events| HttpResponse::Ok().json(events))
}

#[actix_web::post("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn counter_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    body: Json<Proposal>,
    id: Identity,
) -> impl Responder {
    let PathSubscriptionProposal {
        subscription_id,
        proposal_id,
    } = path.into_inner();
    let proposal = body.into_inner();
    market
        .provider_engine
        .counter_proposal(&subscription_id, &proposal_id, &proposal)
        .await
        .map(|proposal_id| HttpResponse::Ok().json(proposal_id))
}

#[actix_web::get("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn get_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::delete("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn reject_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::post("/agreements/{agreement_id}/approve")]
async fn approve_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    query: Query<QueryTimeout>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::post("/agreements/{agreement_id}/reject")]
async fn reject_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::post("/agreements/{agreement_id}/terminate")]
async fn terminate_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::get("/agreements/{agreement_id}")]
async fn get_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}
