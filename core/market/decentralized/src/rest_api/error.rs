use actix_web::{HttpResponse, ResponseError};

use ya_client::model::ErrorMessage;

use crate::{
    market::MarketError,
    matcher::error::{DemandError, MatcherError, OfferError, ResolverError},
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
            MarketError::OfferError(e) => e.error_response(),
            MarketError::DemandError(e) => e.error_response(),
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
            MatcherError::ResolverError(e) => e.error_response(),
            MatcherError::UnexpectedError(e) => {
                HttpResponse::InternalServerError().json(ErrorMessage::new(e.to_string()))
            }
        }
    }
}

impl ResponseError for NegotiationError {}

impl ResponseError for ResolverError {
    fn error_response(&self) -> HttpResponse {
        match self {
            _ => HttpResponse::InternalServerError().json(ErrorMessage::new(self.to_string())),
        }
    }
}

impl ResponseError for DemandError {
    fn error_response(&self) -> HttpResponse {
        match self {
            DemandError::NotFound(e) => {
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
            OfferError::SubscriptionValidation(e) => HttpResponse::BadRequest().json(msg),
            OfferError::NotFound(e) => HttpResponse::NotFound().json(msg),
            OfferError::AlreadyUnsubscribed(_) | OfferError::Expired(_) => {
                HttpResponse::Gone().json(msg)
            }
            _ => HttpResponse::InternalServerError().json(msg),
        }
    }
}
