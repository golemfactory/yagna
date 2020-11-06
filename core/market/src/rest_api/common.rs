use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Responder, Scope};
use chrono::{TimeZone, Utc};
use std::sync::Arc;

use ya_service_api_web::middleware::Identity;
use ya_std_utils::LogErr;

use super::PathAgreement;
use crate::db::model::OwnerType;
use crate::market::MarketService;
use crate::negotiation::error::AgreementError;
use crate::rest_api::QueryAgreementEvents;

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
    path: Path<QueryAgreementEvents>,
    id: Identity,
) -> impl Responder {
    let timeout: f32 = path.timeout;
    let after_timestamp = path
        .after_timestamp
        .unwrap_or(Utc.ymd(1970, 1, 1).and_hms(0, 0, 0));

    market
        .requestor_engine
        .query_agreement_events(
            &path.app_session_id,
            timeout,
            path.max_events,
            after_timestamp,
            &id,
        )
        .await
        .log_err()
        .map(|events| HttpResponse::Ok().json(events))
}
