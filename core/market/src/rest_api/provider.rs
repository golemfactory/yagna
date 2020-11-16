use actix_web::web::{Data, Json, Path, Query};
use actix_web::{HttpResponse, Responder, Scope};
use std::sync::Arc;

use ya_client::model::market::DemandOfferBase;
use ya_service_api_web::middleware::Identity;
use ya_std_utils::LogErr;

use crate::db::model::OwnerType;
use crate::market::MarketService;

use super::{
    PathAgreement, PathSubscription, PathSubscriptionProposal, QueryTimeout, QueryTimeoutMaxEvents,
};

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
}

#[actix_web::post("/offers")]
async fn subscribe(
    market: Data<Arc<MarketService>>,
    body: Json<DemandOfferBase>,
    id: Identity,
) -> impl Responder {
    market
        .subscribe_offer(&body.into_inner(), &id)
        .await
        .log_err()
        .map(|id| HttpResponse::Created().json(id))
}

#[actix_web::get("/offers")]
async fn get_offers(market: Data<Arc<MarketService>>, id: Identity) -> impl Responder {
    market
        .get_offers(Some(id))
        .await
        .log_err()
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
        .log_err()
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
        .log_err()
        .map(|events| HttpResponse::Ok().json(events))
}

#[actix_web::post("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn counter_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    body: Json<DemandOfferBase>,
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
        .log_err()
        .map(|proposal_id| HttpResponse::Ok().json(proposal_id))
}

#[actix_web::get("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn get_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    _id: Identity,
) -> impl Responder {
    // TODO: Authorization
    let PathSubscriptionProposal {
        subscription_id,
        proposal_id,
    } = path.into_inner();

    market
        .provider_engine
        .common
        .get_client_proposal(Some(subscription_id), &proposal_id)
        .await
        .map(|proposal| HttpResponse::Ok().json(proposal))
}

#[actix_web::delete("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn reject_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    id: Identity,
) -> impl Responder {
    let PathSubscriptionProposal {
        subscription_id,
        proposal_id,
    } = path.into_inner();

    market
        .provider_engine
        .reject_proposal(&subscription_id, &proposal_id, &id)
        .await
        .log_err()
        .map(|_| HttpResponse::NoContent().finish())
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
        .log_err()
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
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    id: Identity,
    reason: Option<String>,
) -> impl Responder {
    let agreement_id = path.into_inner().to_id(OwnerType::Provider)?;
    market
        .provider_engine
        .terminate_agreement(id, &agreement_id, reason)
        .await
        .log_err()
        .map(|_| HttpResponse::Ok().finish())
}
