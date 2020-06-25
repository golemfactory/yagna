use actix_web::{HttpResponse, ResponseError};

use ya_client::model::ErrorMessage;

use crate::{
    db::dao::UnsubscribeError,
    market::MarketError,
    matcher::{DemandError, MatcherError, OfferError},
    negotiation::NegotiationError,
};

impl From<MarketError> for actix_web::HttpResponse {
    fn from(e: MarketError) -> Self {
        e.error_response()
    }
}

impl ResponseError for MarketError {
    fn error_response(&self) -> HttpResponse {
        match self {
            MarketError::Matcher(e) => e.error_response(),
            MarketError::Negotiation(e) => e.error_response(),
            MarketError::InternalError(e) => HttpResponse::InternalServerError().json(e),
        }
    }
}

impl ResponseError for MatcherError {
    fn error_response(&self) -> HttpResponse {
        match self {
            MatcherError::DemandError(e) => e.error_response(),
            MatcherError::OfferError(e) => e.error_response(),
            MatcherError::SubscriptionValidation(e) => {
                HttpResponse::BadRequest().json(ErrorMessage::new(e.to_string()))
            }
            MatcherError::UnexpectedError(e) => {
                HttpResponse::InternalServerError().json(ErrorMessage::new(e.to_string()))
            }
        }
    }
}

impl ResponseError for NegotiationError {}

impl ResponseError for DemandError {
    fn error_response(&self) -> HttpResponse {
        match self {
            DemandError::DemandNotExists(e) => {
                HttpResponse::NotFound().json(ErrorMessage::new(self.to_string()))
            }
            _ => HttpResponse::InternalServerError().json(ErrorMessage::new(self.to_string())),
        }
    }
}

impl ResponseError for OfferError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            OfferError::OfferNotExists(e) => {
                HttpResponse::NotFound().json(ErrorMessage::new(self.to_string()))
            }
            OfferError::UnsubscribeOfferFailure(e, id) => match e {
                UnsubscribeError::OfferNotFound | UnsubscribeError::AlreadyUnsubscribed => {
                    HttpResponse::NotFound().json(msg)
                }
                UnsubscribeError::OfferExpired => HttpResponse::Gone().json(msg),
                UnsubscribeError::DatabaseError(_) => HttpResponse::InternalServerError().json(msg),
            },
            _ => HttpResponse::InternalServerError().json(msg),
        }
    }
}
