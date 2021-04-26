use actix_web::web::{Data, Json, Path, Query};
use actix_web::{HttpResponse, Responder, Scope};
use std::sync::Arc;

use ya_client::model::market::{NewOffer, NewProposal, Reason};
use ya_service_api_web::middleware::Identity;
use ya_std_utils::LogErr;

use crate::db::model::Owner;
use crate::market::MarketService;

use super::{PathAgreement, PathSubscription, PathSubscriptionProposal, QueryTimeoutMaxEvents};
use crate::negotiation::ApprovalResult;
use crate::rest_api::QueryTimeoutAppSessionId;
use ya_client::model::ErrorMessage;

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
    body: Json<NewOffer>,
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
        .map(|_| HttpResponse::NoContent())
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
    body: Json<NewProposal>,
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
        .get_client_proposal(Some(&subscription_id), &proposal_id)
        .await
        .map(|proposal| HttpResponse::Ok().json(proposal))
}

#[actix_web::post("/offers/{subscription_id}/proposals/{proposal_id}/reject")]
async fn reject_proposal(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscriptionProposal>,
    id: Identity,
    body: Json<Option<Reason>>,
) -> impl Responder {
    let PathSubscriptionProposal {
        subscription_id,
        proposal_id,
    } = path.into_inner();

    market
        .provider_engine
        .reject_proposal(&subscription_id, &proposal_id, &id, body.into_inner())
        .await
        .log_err()
        .map(|_| HttpResponse::NoContent().finish())
}

#[actix_web::post("/agreements/{agreement_id}/approve")]
async fn approve_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    query: Query<QueryTimeoutAppSessionId>,
    id: Identity,
) -> impl Responder {
    let agreement_id = path.into_inner().to_id(Owner::Provider)?;
    let timeout = query.timeout;
    let session = query.into_inner().app_session_id;
    market
        .provider_engine
        .approve_agreement(id, &agreement_id, session, timeout)
        .await
        .log_err()
        .map(|result| match result {
            ApprovalResult::Approved => HttpResponse::NoContent().finish(),
            _ => HttpResponse::Gone().json(ErrorMessage::new(result)),
        })
}

#[actix_web::post("/agreements/{agreement_id}/reject")]
async fn reject_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    id: Identity,
    body: Json<Option<Reason>>,
) -> impl Responder {
    let agreement_id = path.into_inner().to_id(Owner::Provider)?;
    market
        .provider_engine
        .reject_agreement(&id, &agreement_id, body.into_inner())
        .await
        .log_err()
        .map(|_| HttpResponse::Ok().finish())
}
