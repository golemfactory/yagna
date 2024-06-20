use actix_web::http::header;
use actix_web::http::header::CacheDirective;
use actix_web::web::{Data, Json, Path, Query};
use actix_web::{web, Either, HttpResponse, Responder, Scope};
use chrono::{TimeZone, Utc};
use std::convert::TryInto;
use std::sync::Arc;

use ya_client::model::market::scan::NewScan;
use ya_client::model::market::{Offer, Reason};
use ya_service_api_web::middleware::Identity;
use ya_service_bus::timeout::IntoTimeoutFuture;
use ya_std_utils::LogErr;

use super::{PathAgreement, QueryScanEvents};
use crate::db::model::Owner;
use crate::market::MarketService;
use crate::negotiation::error::{AgreementError, ScanError};
use crate::negotiation::{ScanId, ScannerSet};
use crate::rest_api::{QueryAgreementEvents, QueryAgreementList};
use futures::prelude::*;
use tracing::Level;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .service(list_agreements)
        .service(collect_agreement_events)
        .service(get_agreement)
        .service(terminate_agreement)
        .service(get_agreement_terminate_reason)
        .service(scan_begin)
        .service(scan_collect)
        .service(scan_end)
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
        Err(AgreementError::Internal("We found ".to_string()))
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
        .unwrap_or_else(|| Utc.with_ymd_and_hms(2016, 11, 11, 15, 12, 0).unwrap());

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

#[actix_web::get("/agreements/{agreement_id}/terminate/reason")]
async fn get_agreement_terminate_reason(
    market: Data<Arc<MarketService>>,
    path: Path<PathAgreement>,
    id: Identity,
) -> impl Responder {
    let client_agreement_id = path.into_inner().agreement_id;
    market
        .get_terminate_reason(id, client_agreement_id)
        .await
        .log_err()
        .map(|reason| HttpResponse::Ok().json(reason))
}

#[actix_web::post("/scan")]
async fn scan_begin(
    id: Identity,
    Json(spec): Json<NewScan>,
    scan_set: Data<ScannerSet>,
) -> Result<HttpResponse, ScanError> {
    let id = scan_set.begin(id.identity, spec)?;
    Ok(HttpResponse::Created().json(id))
}

#[actix_web::get("/scan/{scanId}/events")]
async fn scan_collect(
    id: Identity,
    path: Path<(String,)>,
    query: Query<QueryScanEvents>,
    scan_set: Data<ScannerSet>,
    accept: web::Header<header::Accept>,
) -> Result<Either<HttpResponse, Json<Vec<Offer>>>, ScanError> {
    let scan_id: ScanId = path.0.parse()?;
    let owner_id = id.identity;

    if let Some(peer_id) = query.0.peer_id {
        let max_events =
            query
                .max_events
                .unwrap_or(500)
                .try_into()
                .map_err(|e| ScanError::BadRequest {
                    field: "maxEvents".into(),
                    cause: anyhow::Error::new(e),
                })?;

        let data = scan_set
            .direct_offers(owner_id, scan_id.clone(), peer_id, max_events)
            .timeout(Some(query.timeout))
            .await;
        return match data {
            Err(_e) => Err(ScanError::FetchTimeout),
            Ok(Err(e)) => Err(e),
            Ok(Ok(v)) => Ok(Either::Right(Json(v))),
        }
        .inspect_err(|e| {
            tracing::event!(
                Level::ERROR,
                entity = "scan",
                scan_id = display(&scan_id),
                peer_id = display(peer_id),
                "{e}"
            )
        });
    }

    if accept.preference() == mime::TEXT_EVENT_STREAM {
        // to check if iterator is valid.
        scan_set.collect(owner_id, scan_id.clone(), 0).await?;

        let offers = stream::try_unfold((), move |v| {
            let scan_set = scan_set.clone();
            let scan_id = scan_id.clone();

            async move {
                let offers = scan_set.collect(owner_id, scan_id, 100).await?;

                Ok::<_, ScanError>(Some((
                    stream::iter(offers.into_iter().map(Ok::<_, ScanError>)),
                    v,
                )))
            }
        })
        .try_flatten()
        .map_ok(|offer| {
            let json = serde_json::to_string(&offer)
                .unwrap_or("{}".to_string())
                .replace('\n', "\ndata: ");

            web::Bytes::from(format!("event: offer\ndata: {json}\n\n"))
        });

        Ok(Either::Left(
            HttpResponse::Ok()
                .content_type(mime::TEXT_EVENT_STREAM)
                .append_header(header::CacheControl(vec![CacheDirective::NoCache]))
                .streaming(offers),
        ))
    } else {
        match scan_set
            .collect(
                owner_id,
                scan_id,
                query.max_events.unwrap_or(500).try_into().unwrap(),
            )
            .timeout(Some(query.timeout))
            .await
        {
            Err(_e) => Ok(Either::Right(Json(Vec::new()))),
            Ok(Err(e)) => Err(e),
            Ok(Ok(v)) => Ok(Either::Right(Json(v))),
        }
    }
}

#[actix_web::delete("/scan/{subscriptionId}")]
async fn scan_end(
    id: Identity,
    path: Path<(String,)>,
    scan_set: Data<ScannerSet>,
) -> Result<HttpResponse, ScanError> {
    let scan_id = path.0.parse()?;

    scan_set.end(id.identity, scan_id).await?;
    Ok(HttpResponse::NoContent().finish())
}
