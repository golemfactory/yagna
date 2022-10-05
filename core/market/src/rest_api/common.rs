use actix_web::web::{Data, Json, Path, Query};
use actix_web::{HttpResponse, Responder, Scope};
use chrono::{TimeZone, Utc};
use std::sync::Arc;

use ya_client::model::market::Reason;
use ya_service_api_web::middleware::Identity;
use ya_std_utils::LogErr;

use super::PathAgreement;
use crate::db::model::Owner;
use crate::market::MarketService;
use crate::negotiation::error::AgreementError;
use crate::rest_api::{QueryAgreementEvents, QueryAgreementList};

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .service(list_agreements)
        .service(collect_agreement_events)
        .service(get_agreement)
        .service(terminate_agreement)
}

#[actix_web::get("/agreements")]
async fn list_agreements(
    market: Data<Arc<MarketService>>,
    query: Query<QueryAgreementList>,
    id: Identity,
) -> impl Responder {
    let query = query.into_inner();

    market
        .list_agreements(
            &id,
            query.state.map(Into::into),
            query.before_date,
            query.after_date,
            query.app_session_id,
        )
        .await
        .map(|list| HttpResponse::Ok().json(list))
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
    let r_agreement_id = path.to_id(Owner::Requestor)?;
    let p_agreement_id = r_agreement_id.clone().swap_owner();

    let r_result = market.get_agreement(&r_agreement_id, &id).await;
    let p_result = market.get_agreement(&p_agreement_id, &id).await;

    if p_result.is_err() && r_result.is_err() {
        Err(AgreementError::NotFound(path.agreement_id)).log_err()
    } else if r_result.is_err() {
        p_result.map(|agreement| HttpResponse::Ok().json(agreement))
    } else if p_result.is_err() {
        r_result.map(|agreement| HttpResponse::Ok().json(agreement))
    } else {
        // Both calls shouldn't return Agreement.
        Err(AgreementError::Internal(format!("We found ")))
    }
}

#[actix_web::get("/agreementEvents")]
async fn collect_agreement_events(
    market: Data<Arc<MarketService>>,
    query: Query<QueryAgreementEvents>,
    id: Identity,
) -> impl Responder {
    let timeout: f32 = query.timeout;
    let after_timestamp = query
        .after_timestamp
        .unwrap_or_else(|| Utc.ymd(2016, 11, 11).and_hms(15, 12, 0));

    market
        .query_agreement_events(
            &query.app_session_id,
            timeout,
            query.max_events,
            after_timestamp,
            &id,
        )
        .await
        .log_err()
        .map(|events| HttpResponse::Ok().json(events))
}

#[actix_web::post("/agreements/{agreement_id}/terminate")]
async fn terminate_agreement(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    id: Identity,
    body: Json<Option<Reason>>,
) -> impl Responder {
    let client_agreement_id = path.into_inner().agreement_id;
    market
        .terminate_agreement(id, client_agreement_id, body.into_inner())
        .await
        .log_err()
        .map(|_| HttpResponse::Ok().finish())
}
