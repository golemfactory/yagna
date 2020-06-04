use actix_web::web::{Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use std::sync::Arc;

use super::response;
use super::{
    PathAgreement, PathSubscription, PathSubscriptionProposal, QueryTimeout, QueryTimeoutMaxEvents,
};
use crate::market::MarketService;

use ya_client::model::market::{Agreement, AgreementProposal, Offer, Proposal};
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
}

#[actix_web::post("/offers")]
async fn subscribe(
    market: Data<Arc<MarketService>>,
    body: Json<Offer>,
    id: Identity,
) -> HttpResponse {
    match market.subscribe_offer(&body.into_inner(), id).await {
        Ok(subscription_id) => response::created("Ok"),
        // TODO: Translate MarketError to better HTTP response.
        Err(error) => response::server_error(&format!("{}", error)),
    }
}

#[actix_web::get("/offers")]
async fn get_offers(market: Data<Arc<MarketService>>, id: Identity) -> HttpResponse {
    response::not_implemented()
}

#[actix_web::delete("/offers/{subscription_id}")]
async fn unsubscribe(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscription>,
    id: Identity,
) -> HttpResponse {
    let subscription_id = path.into_inner().subscription_id;
    match market.matcher.get_offer(subscription_id.clone()).await {
        Ok(Some(_offer)) => response::ok(subscription_id),
        Ok(None) => response::not_found(),
        // TODO: Translate MatcherError to better HTTP response.
        Err(error) => response::server_error(&format!("{}", error)),
    }
}

#[actix_web::get("/offers/{subscription_id}/events")]
async fn collect(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscription>,
    query: Query<QueryTimeoutMaxEvents>,
    id: Identity,
) -> HttpResponse {
    response::not_implemented()
}

#[actix_web::post("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn counter_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    body: Json<Proposal>,
    id: Identity,
) -> HttpResponse {
    response::not_implemented()
}

#[actix_web::get("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn get_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    id: Identity,
) -> HttpResponse {
    response::not_implemented()
}

#[actix_web::delete("/offers/{subscription_id}/proposals/{proposal_id}")]
async fn reject_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    id: Identity,
) -> HttpResponse {
    response::not_implemented()
}

#[actix_web::post("/agreements/{agreement_id}/approve")]
async fn approve_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    query: Query<QueryTimeout>,
    id: Identity,
) -> HttpResponse {
    response::not_implemented()
}

#[actix_web::post("/agreements/{agreement_id}/reject")]
async fn reject_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    id: Identity,
) -> HttpResponse {
    response::not_implemented()
}
