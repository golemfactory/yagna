use actix_web::{HttpResponse, ResponseError};

use ya_client::model::ErrorMessage;

use crate::{
    db::dao::TakeEventsError,
    market::MarketError,
    matcher::error::{
        DemandError, MatcherError, ModifyOfferError, QueryOfferError, QueryOffersError,
        ResolverError, SaveOfferError,
    },
    negotiation::error::{
        AgreementError, NegotiationError, ProposalError, QueryEventsError, WaitForApprovalError,
    },
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
            MarketError::QueryOfferError(e) => e.error_response(),
            MarketError::QueryOffersError(e) => e.error_response(),
            MarketError::DemandError(e) => e.error_response(),
            MarketError::Negotiation(e) => e.error_response(),
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
            ProposalError::NotFound(..)
            | ProposalError::QueryOfferError(QueryOfferError::NotFound(..))
            | ProposalError::DemandError(DemandError::NotFound(..)) => {
                HttpResponse::NotFound().json(msg)
            }
            _ => HttpResponse::InternalServerError().json(msg),
        }
    }
}

impl ResponseError for AgreementError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            AgreementError::NotFound(_) => HttpResponse::NotFound().json(msg),
            AgreementError::Confirmed(_)
            | AgreementError::Cancelled(_)
            | AgreementError::Approved(_)
            | AgreementError::Proposed(_) => HttpResponse::Conflict().json(msg),
            AgreementError::Rejected(_)
            | AgreementError::Expired(_)
            | AgreementError::Terminated(_) => HttpResponse::Gone().json(msg),
            AgreementError::NoNegotiations(_)
            | AgreementError::ProposalNotFound(..)
            | AgreementError::InvalidSubscriptionId(..) => HttpResponse::BadRequest().json(msg),
            AgreementError::GetProposal(..)
            | AgreementError::Save(..)
            | AgreementError::Get(..)
            | AgreementError::Update(..)
            | AgreementError::Protocol(_)
            | AgreementError::ProtocolApprove(_) => HttpResponse::InternalServerError().json(msg),
        }
    }
}

impl ResponseError for WaitForApprovalError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            WaitForApprovalError::NotFound(_) => HttpResponse::NotFound().json(msg),
            WaitForApprovalError::Expired(_) => HttpResponse::Gone().json(msg),
            WaitForApprovalError::Terminated(_) | WaitForApprovalError::NotConfirmed(_) => {
                HttpResponse::BadRequest().json(msg)
            }
            WaitForApprovalError::Timeout(_) => HttpResponse::RequestTimeout().json(msg),
            WaitForApprovalError::InternalError(_) | WaitForApprovalError::Get(..) => {
                HttpResponse::InternalServerError().json(msg)
            }
        }
    }
}
