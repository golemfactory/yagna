use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Responder, Scope};
use std::sync::Arc;

use ya_service_api_web::middleware::Identity;
use ya_std_utils::LogErr;

use super::PathAgreement;
use crate::db::model::{AppSessionId, OwnerType};
use crate::market::MarketService;
use crate::negotiation::error::AgreementError;
use chrono::Utc;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .service(get_agreement)
        .service(collect_agreement_events)
}

#[actix_web::get("/agreements/{agreement_id}")]
async fn get_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    id: Identity,
) -> impl Responder {
    // We don't know, if we are requestor or provider. Try to get Agreement for both sides
    // and check, if any will be returned. Note that we won't get Agreement if we aren't
    // owner, so here is no danger, that Provider gets Requestor's Offer and opposite.
    let path = path.into_inner();
    let r_agreement_id = path.clone().to_id(OwnerType::Requestor)?;
    let p_agreement_id = path.to_id(OwnerType::Provider)?;

    let r_result = market.get_agreement(&r_agreement_id, &id).await;
    let p_result = market.get_agreement(&p_agreement_id, &id).await;

    if p_result.is_err() && r_result.is_err() {
        Err(AgreementError::NotFound(r_agreement_id)).log_err()
    } else if r_result.is_err() {
        p_result.map(|agreement| HttpResponse::Ok().json(agreement))
    } else if p_result.is_err() {
        r_result.map(|agreement| HttpResponse::Ok().json(agreement))
    } else {
        // Both calls shouldn't return Agreement.
        Err(AgreementError::Internal(format!("We found ")))
    }
}

#[actix_web::get("/agreements/events")]
async fn collect_agreement_events(
    market: Data<Arc<MarketService>>,
    id: Identity,
) -> impl Responder {
    // TODO: Should come from parameters
    let session_id: AppSessionId = None;
    let timeout: f32 = 0.0;
    let max_events = Some(10);
    let after_timestamp = Utc::now();

    market
        .provider_engine
        .common
        .query_agreement_events(&session_id, timeout, max_events, after_timestamp, &id)
        .await
        .log_err()
        .map(|events| HttpResponse::Ok().json(events))
}
