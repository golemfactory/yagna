use actix_web::web::{Data, Json, Path, Query};
use actix_web::{HttpResponse, Responder, Scope};
use std::sync::Arc;

use super::{
    PathAgreement, PathSubscription, PathSubscriptionProposal, QueryTimeout, QueryTimeoutMaxEvents,
};
use crate::market::MarketService;

use ya_client::model::market::{AgreementProposal, Demand, Proposal};
use ya_service_api_web::middleware::Identity;

// This file contains market REST endpoints. Responsibility of these functions
// is calling respective functions in market modules and mapping return values
// to http responses. No market logic is allowed here.

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .service(subscribe)
        .service(get_demands)
        .service(unsubscribe)
        .service(collect)
        .service(counter_proposal)
        .service(get_proposal)
        .service(reject_proposal)
        .service(create_agreement)
        .service(get_agreement)
        .service(confirm_agreement)
        .service(wait_for_approval)
        .service(cancel_agreement)
        .service(terminate_agreement)
}

#[actix_web::post("/demands")]
async fn subscribe(
    market: Data<Arc<MarketService>>,
    body: Json<Demand>,
    id: Identity,
) -> impl Responder {
    market
        .subscribe_demand(&body.into_inner(), &id)
        .await
        .map(|id| HttpResponse::Created().json(id))
}

#[actix_web::get("/demands")]
async fn get_demands(market: Data<Arc<MarketService>>, id: Identity) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::delete("/demands/{subscription_id}")]
async fn unsubscribe(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscription>,
    id: Identity,
) -> impl Responder {
    let subscription_id = path.into_inner().subscription_id;
    market
        .unsubscribe_demand(&subscription_id, &id)
        .await
        .map(|x| HttpResponse::Ok().json("Ok"))
}

#[actix_web::get("/demands/{subscription_id}/events")]
async fn collect(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscription>,
    query: Query<QueryTimeoutMaxEvents>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::post("/demands/{subscription_id}/proposals/{proposal_id}")]
async fn counter_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    body: Json<Proposal>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::get("/demands/{subscription_id}/proposals/{proposal_id}")]
async fn get_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::delete("/demands/{subscription_id}/proposals/{proposal_id}")]
async fn reject_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::post("/agreements")]
async fn create_agreement(
    market: Data<Arc<MarketService>>,
    body: Json<AgreementProposal>,
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

#[actix_web::post("/agreements/{agreement_id}/confirm")]
async fn confirm_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::post("/agreements/{agreement_id}/wait")]
async fn wait_for_approval(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    query: Query<QueryTimeout>,
    id: Identity,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish()
}

#[actix_web::delete("/agreements/{agreement_id}")]
async fn cancel_agreement(
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
