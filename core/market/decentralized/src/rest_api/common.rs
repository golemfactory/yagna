use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Responder};
use std::sync::Arc;

use ya_service_api_web::middleware::Identity;
use ya_std_utils::ResultExt;

use super::PathAgreement;
use crate::db::model::OwnerType;
use crate::market::MarketService;

#[actix_web::get("/agreements/{agreement_id}")]
async fn get_agreement(
    market: Data<Arc<MarketService>>,
    body: Path<PathAgreement>,
    id: Identity,
) -> impl Responder {
    /// OwnerType::Requestor will be checked internally
    let agreement_id = body.into_inner().to_id(OwnerType::Requestor)?;
    market
        .get_agreement(&agreement_id, &id)
        .await
        .inspect_err(|e| log::error!("[GetAgreement] {}", e))
        .map(|agreement| HttpResponse::Ok().json(agreement))
}
