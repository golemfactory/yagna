use actix_web::web::{Data, Json, Path, Query};
use actix_web::{HttpResponse, Responder, Scope};
use std::sync::Arc;

use ya_client::model::market::{Offer, Proposal};
use ya_service_api_web::middleware::Identity;
use ya_std_utils::ResultExt;

use crate::market::MarketService;

use super::common::*;
use super::{
    PathAgreement, PathSubscription, PathSubscriptionProposal, QueryTimeout, QueryTimeoutMaxEvents,
};
use crate::db::model::OwnerType;

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
        .inspect_err(|e| log::error!("[SubscribeOffer] {}", e))
        .map(|id| HttpResponse::Created().json(id))
}

#[actix_web::get("/offers")]
async fn get_offers(market: Data<Arc<MarketService>>, id: Identity) -> impl Responder {
    market
        .get_offers(Some(id))
        .await
        .inspect_err(|e| log::error!("[GetOffer] {}", e))
        .map(|offers| HttpResponse::Ok().json(offers))
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
        .inspect_err(|e| log::error!("[UnsubscribeOffer] {}", e))
        .map(|_| HttpResponse::Ok().json("Ok"))
}

#[actix_web::get("/offers/{subscription_id}/events")]
async fn collect(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscription>,
    query: Query<QueryTimeoutMaxEvents>,
    _id: Identity,
) -> impl Responder {
    let subscription_id = path.into_inner().subscription_id;
    let timeout = query.timeout;
    let max_events = query.max_events;
    market
        .provider_engine
        .query_events(&subscription_id, timeout, max_events)
        .await
        .inspect_err(|e| log::error!("[QueryEvents] {}", e))
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
        .counter_proposal(&subscription_id, &proposal_id, &proposal, &id)
        .await
        .inspect_err(|e| log::error!("[CounterProposal] {}", e))
        .map(|proposal_id| HttpResponse::Ok().json(proposal_id))
}

#[actix_web::get("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn get_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    id: Identity,
) -> impl Responder {
    let PathSubscriptionProposal { proposal_id, .. } = path.into_inner();
    market
        .get_proposal(&proposal_id, &id)
        .await
        .map(|proposal| HttpResponse::Ok().json(proposal))
}

#[actix_web::delete("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn reject_proposal(
    _market: Data<Arc<MarketService>>,
    _path: Path<PathSubscriptionProposal>,
    _id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::post("/agreements/{agreement_id}/approve")]
async fn approve_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    query: Query<QueryTimeout>,
    id: Identity,
) -> impl Responder {
    let agreement_id = path.into_inner().to_id(OwnerType::Provider)?;
    let timeout = query.timeout;
    market
        .provider_engine
        .approve_agreement(id, &agreement_id, timeout)
        .await
        .inspect_err(|e| log::error!("[ApproveAgreement] {}", e))
        .map(|_| HttpResponse::NoContent().finish())
}

#[actix_web::post("/agreements/{agreement_id}/reject")]
async fn reject_agreement(
    _market: Data<Arc<MarketService>>,
    _path: Path<PathAgreement>,
    _id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::post("/agreements/{agreement_id}/terminate")]
async fn terminate_agreement(
    _market: Data<Arc<MarketService>>,
    _path: Path<PathAgreement>,
    _id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}
