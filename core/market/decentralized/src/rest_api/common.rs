use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Responder};
use std::fmt::Display;
use std::sync::Arc;

use ya_service_api_web::middleware::Identity;

use super::PathAgreement;
use crate::market::MarketService;

pub trait ResultEnhancements<T, E> {
    fn log_err(self) -> Result<T, E>;
}

impl<T, E> ResultEnhancements<T, E> for Result<T, E>
where
    E: Display,
{
    fn log_err(self) -> Result<T, E> {
        match self {
            Ok(content) => Ok(content),
            Err(e) => {
                log::error!("{}", &e);
                Err(e)
            }
        }
    }
}

#[actix_web::get("/agreements/{agreement_id}")]
async fn get_agreement(
    market: Data<Arc<MarketService>>,
    body: Path<PathAgreement>,
    id: Identity,
) -> impl Responder {
    let agreement_id = body.into_inner().agreement_id;
    market
        .get_agreement(&agreement_id, &id)
        .await
        .log_err()
        .map(|agreement| HttpResponse::Ok().json(agreement))
}
