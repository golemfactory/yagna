use actix_web::{HttpResponse, ResponseError};

use ya_client::model::ErrorMessage;

use crate::db::dao::TakeEventsError;
use crate::matcher::error::{QueryDemandsError, QueryOffersError, SaveOfferError};
use crate::negotiation::error::{
    AgreementError, AgreementStateError, ProposalError, WaitForApprovalError,
};
use crate::{
    market::MarketError,
    matcher::error::{DemandError, MatcherError, ModifyOfferError, QueryOfferError, ResolverError},
    negotiation::error::{NegotiationError, QueryEventsError},
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
            MarketError::QueryDemandsError(e) => e.error_response(),
            MarketError::QueryOfferError(e) => e.error_response(),
            MarketError::QueryOffersError(e) => e.error_response(),
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
            MatcherError::QueryOffersError(e) => e.error_response(),
            MatcherError::QueryOfferError(e) => e.error_response(),
            MatcherError::SaveOfferError(e) => e.error_response(),
            MatcherError::ModifyOfferError(e) => e.error_response(),
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
            DemandError::NotFound(_) => {
                HttpResponse::NotFound().json(ErrorMessage::new(self.to_string()))
            }
            _ => HttpResponse::InternalServerError().json(ErrorMessage::new(self.to_string())),
        }
    }
}

impl ResponseError for QueryDemandsError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::InternalServerError().json(ErrorMessage::new(self.to_string()))
    }
}

impl ResponseError for QueryOffersError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::InternalServerError().json(ErrorMessage::new(self.to_string()))
    }
}

impl ResponseError for QueryOfferError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            QueryOfferError::NotFound(_) => HttpResponse::NotFound().json(msg),
            _ => HttpResponse::InternalServerError().json(msg),
        }
    }
}

impl ResponseError for SaveOfferError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            SaveOfferError::Unsubscribed(_) | SaveOfferError::Expired(_) => {
                HttpResponse::Gone().json(msg)
            }
            _ => HttpResponse::InternalServerError().json(msg),
        }
    }
}

impl ResponseError for ModifyOfferError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            ModifyOfferError::NotFound(_) => HttpResponse::NotFound().json(msg),
            ModifyOfferError::Unsubscribed(_) | ModifyOfferError::Expired(_) => {
                HttpResponse::Gone().json(msg)
            }
            _ => HttpResponse::InternalServerError().json(msg),
        }
    }
}

impl ResponseError for QueryEventsError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            QueryEventsError::Unsubscribed(_)
            | QueryEventsError::TakeEventsError(TakeEventsError::SubscriptionNotFound(_))
            | QueryEventsError::TakeEventsError(TakeEventsError::SubscriptionExpired(_)) => {
                HttpResponse::NotFound().json(msg)
            }
            QueryEventsError::InvalidSubscriptionId(_) | QueryEventsError::InvalidMaxEvents(_) => {
                HttpResponse::BadRequest().json(msg)
            }
            _ => HttpResponse::InternalServerError().json(msg),
        }
    }
}

impl ResponseError for ProposalError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            _ => HttpResponse::InternalServerError().json(msg),
        }
    }
}

impl ResponseError for AgreementError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            AgreementError::NotFound(_) => HttpResponse::NotFound().json(msg),
            AgreementError::AgreementExists(_, _) => HttpResponse::Conflict().json(msg),
            AgreementError::InvalidState(e) => match e {
                AgreementStateError::Confirmed(_)
                | AgreementStateError::Cancelled(_)
                | AgreementStateError::Approved(_)
                | AgreementStateError::Proposed(_) => HttpResponse::Conflict().json(msg),
                AgreementStateError::Rejected(_)
                | AgreementStateError::Expired(_)
                | AgreementStateError::Terminated(_) => HttpResponse::Gone().json(msg),
            },
            AgreementError::NoNegotiations(_)
            | AgreementError::OwnProposal(..)
            | AgreementError::ProposalNotFound(..)
            | AgreementError::ProposalCountered(..)
            | AgreementError::InvalidSubscriptionId(..) => HttpResponse::BadRequest().json(msg),
            AgreementError::GetProposal(..)
            | AgreementError::Save(..)
            | AgreementError::Get(..)
            | AgreementError::Update(..)
            | AgreementError::Protocol(_)
            | AgreementError::ProtocolCreate(_)
            | AgreementError::ProtocolApprove(_)
            | AgreementError::InternalError(_) => HttpResponse::InternalServerError().json(msg),
        }
    }
}

impl ResponseError for WaitForApprovalError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            WaitForApprovalError::NotFound(_) => HttpResponse::NotFound().json(msg),
            WaitForApprovalError::AgreementExpired(_) => HttpResponse::Gone().json(msg),
            WaitForApprovalError::AgreementTerminated(_)
            | WaitForApprovalError::AgreementNotConfirmed(_) => {
                HttpResponse::BadRequest().json(msg)
            }
            WaitForApprovalError::Timeout(_) => HttpResponse::RequestTimeout().json(msg),
            WaitForApprovalError::InternalError(_) | WaitForApprovalError::FailedGetFromDb(..) => {
                HttpResponse::InternalServerError().json(msg)
            }
        }
    }
}
