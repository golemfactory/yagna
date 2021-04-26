use actix_web::web::{Data, Json, Path, Query};
use actix_web::{HttpResponse, Responder, Scope};
use std::str::FromStr;
use std::sync::Arc;

use ya_client::model::market::{AgreementProposal, NewDemand, NewProposal, Reason};
use ya_client::model::ErrorMessage;
use ya_service_api_web::middleware::Identity;
use ya_std_utils::LogErr;

use crate::db::model::Owner;
use crate::market::MarketService;

use super::{
    PathAgreement, PathSubscription, PathSubscriptionProposal, ProposalId, QueryTimeout,
    QueryTimeoutMaxEvents,
};
use crate::negotiation::ApprovalStatus;
use crate::rest_api::QueryAppSessionId;

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
        .service(confirm_agreement)
        .service(wait_for_approval)
        .service(cancel_agreement)
}

#[actix_web::post("/demands")]
async fn subscribe(
    market: Data<Arc<MarketService>>,
    body: Json<NewDemand>,
    id: Identity,
) -> impl Responder {
    market
        .subscribe_demand(&body.into_inner(), &id)
        .await
        .log_err()
        .map(|id| HttpResponse::Created().json(id))
}

#[actix_web::get("/demands")]
async fn get_demands(market: Data<Arc<MarketService>>, id: Identity) -> impl Responder {
    market
        .get_demands(Some(id))
        .await
        .map(|demands| HttpResponse::Ok().json(demands))
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
        .log_err()
        .map(|_| HttpResponse::NoContent())
}

#[actix_web::get("/demands/{subscription_id}/events")]
async fn collect(
    market: Data<Arc<MarketService>>,
    path: Path<PathSubscription>,
    query: Query<QueryTimeoutMaxEvents>,
    _id: Identity, // TODO: use it
) -> impl Responder {
    let subscription_id = path.into_inner().subscription_id;
    let timeout = query.timeout;
    let max_events = query.max_events;
    market
        .requestor_engine
        .query_events(&subscription_id, timeout, max_events)
        .await
        .log_err()
        .map(|events| HttpResponse::Ok().json(events))
}

#[actix_web::post("/demands/{subscription_id}/proposals/{proposal_id}")]
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
        .requestor_engine
        .counter_proposal(&subscription_id, &proposal_id, &proposal, &id)
        .await
        .log_err()
        .map(|proposal_id| HttpResponse::Ok().json(proposal_id))
}

#[actix_web::get("/demands/{subscription_id}/proposals/{proposal_id}")]
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
        .requestor_engine
        .common
        .get_client_proposal(Some(&subscription_id), &proposal_id)
        .await
        .map(|proposal| HttpResponse::Ok().json(proposal))
}

#[actix_web::post("/demands/{subscription_id}/proposals/{proposal_id}/reject")]
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
        .requestor_engine
        .reject_proposal(&subscription_id, &proposal_id, &id, body.into_inner())
        .await
        .log_err()
        .map(|_| HttpResponse::NoContent().finish())
}

#[actix_web::post("/agreements")]
async fn create_agreement(
    market: Data<Arc<MarketService>>,
    body: Json<AgreementProposal>,
    id: Identity,
) -> impl Responder {
    let proposal_id = ProposalId::from_str(&body.proposal_id)?;
    let valid_to = body.valid_to;
    market
        .requestor_engine
        .create_agreement(id, &proposal_id, valid_to)
        .await
        .log_err()
        .map(|agreement_id| HttpResponse::Ok().json(agreement_id.into_client()))
}

#[actix_web::post("/agreements/{agreement_id}/confirm")]
async fn confirm_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    query: Query<QueryAppSessionId>,
    id: Identity,
) -> impl Responder {
    let agreement_id = path.into_inner().to_id(Owner::Requestor)?;
    market
        .requestor_engine
        .confirm_agreement(id, &agreement_id, query.into_inner().app_session_id)
        .await
        .log_err()
        .map(|_| HttpResponse::NoContent().finish())
}

#[actix_web::post("/agreements/{agreement_id}/wait")]
async fn wait_for_approval(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    query: Query<QueryTimeout>,
    _id: Identity,
) -> impl Responder {
    let agreement_id = path.into_inner().to_id(Owner::Requestor)?;
    let timeout = query.timeout;
    market
        .requestor_engine
        .wait_for_approval(&agreement_id, timeout)
        .await
        .log_err()
        .map(|status| match status {
            ApprovalStatus::Approved => HttpResponse::NoContent().finish(),
            _ => HttpResponse::Gone().json(ErrorMessage::new(status)),
        })
}

#[actix_web::post("/agreements/{agreement_id}/cancel")]
async fn cancel_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    id: Identity,
    body: Json<Option<Reason>>,
) -> impl Responder {
    let agreement_id = path.into_inner().to_id(Owner::Requestor)?;
    market
        .requestor_engine
        .cancel_agreement(&id, &agreement_id, body.into_inner())
        .await
        .log_err()
        .map(|_| HttpResponse::Ok().finish())
}
