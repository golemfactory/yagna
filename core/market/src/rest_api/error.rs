use actix_web::{HttpResponse, ResponseError};

use ya_client::model::ErrorMessage;

use crate::db::dao::{AgreementDaoError, SaveProposalError};
use crate::db::model::AgreementState;
use crate::negotiation::error::{AgreementEventsError, ProposalValidationError};
use crate::protocol::negotiation::error::RejectProposalError;
use crate::{
    db::dao::TakeEventsError,
    market::MarketError,
    matcher::error::{
        DemandError, MatcherError, ModifyOfferError, QueryDemandsError, QueryOfferError,
        QueryOffersError, ResolverError, SaveOfferError,
    },
    negotiation::error::{
        AgreementError, GetProposalError, NegotiationError, ProposalError, QueryEventsError,
        WaitForApprovalError,
    },
};

impl From<MarketError> for actix_web::HttpResponse {
    fn from(e: MarketError) -> Self {
        e.error_response().into()
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
        }
    }
}

impl ResponseError for MatcherError {
    fn error_response(&self) -> HttpResponse {
        match self {
            MatcherError::Demand(e) => e.error_response(),
            MatcherError::QueryOffers(e) => e.error_response(),
            MatcherError::QueryOffer(e) => e.error_response(),
            MatcherError::SaveOffer(e) => e.error_response(),
            MatcherError::ModifyOffer(e) => e.error_response(),
        }
    }
}

impl ResponseError for NegotiationError {}

impl ResponseError for ResolverError {
    fn error_response(&self) -> HttpResponse {
        match self {
            _ => HttpResponse::InternalServerError().json(ErrorMessage::new(self.to_string())),
        }
        .into()
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
        .into()
    }
}

impl ResponseError for QueryDemandsError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::InternalServerError()
            .json(ErrorMessage::new(self.to_string()))
            .into()
    }
}

impl ResponseError for QueryOffersError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::InternalServerError()
            .json(ErrorMessage::new(self.to_string()))
            .into()
    }
}

impl ResponseError for QueryOfferError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            QueryOfferError::NotFound(_) => HttpResponse::NotFound().json(msg),
            _ => HttpResponse::InternalServerError().json(msg),
        }
        .into()
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
        .into()
    }
}

impl ResponseError for ModifyOfferError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            ModifyOfferError::NotFound(_) => HttpResponse::NotFound().json(msg),
            ModifyOfferError::AlreadyUnsubscribed(_) | ModifyOfferError::Expired(_) => {
                HttpResponse::Gone().json(msg)
            }
            _ => HttpResponse::InternalServerError().json(msg),
        }
        .into()
    }
}

impl ResponseError for QueryEventsError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            QueryEventsError::TakeEvents(TakeEventsError::NotFound(_))
            | QueryEventsError::TakeEvents(TakeEventsError::Expired(_)) => {
                HttpResponse::NotFound().json(msg)
            }
            QueryEventsError::InvalidSubscriptionId(_) | QueryEventsError::InvalidMaxEvents(..) => {
                HttpResponse::BadRequest().json(msg)
            }
            _ => HttpResponse::InternalServerError().json(msg),
        }
        .into()
    }
}

impl ResponseError for ProposalError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            ProposalError::Validation(e) => e.error_response(),
            ProposalError::Save(SaveProposalError::AlreadyCountered(..)) => {
                HttpResponse::Gone().json(msg).into()
            }
            ProposalError::Get(e) => e.error_response(),
            ProposalError::Reject(e) => e.error_response(),
            // TODO: get rid of those `_` patterns as they do not break when error is extended
            _ => HttpResponse::InternalServerError().json(msg).into(),
        }
    }
}

impl ResponseError for RejectProposalError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            RejectProposalError::Validation(_) => HttpResponse::BadRequest().json(msg),
            RejectProposalError::Gsb(_)
            | RejectProposalError::Get(_)
            | RejectProposalError::ChangeState(_)
            | RejectProposalError::CallerParse(_) => HttpResponse::InternalServerError().json(msg),
        }
        .into()
    }
}

impl ResponseError for ProposalValidationError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            ProposalValidationError::NoSubscription(_)
            | ProposalValidationError::Unsubscribed(_)
            | ProposalValidationError::NotMatching(_)
            | ProposalValidationError::OwnProposal(_) => HttpResponse::BadRequest().json(msg),
            ProposalValidationError::SubscriptionExpired(_) => HttpResponse::Gone().json(msg),
            ProposalValidationError::Unauthorized(_, _) => HttpResponse::Unauthorized().json(msg),
            ProposalValidationError::Internal(_) => HttpResponse::InternalServerError().json(msg),
        }
        .into()
    }
}

impl ResponseError for GetProposalError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            GetProposalError::NotFound(..) => HttpResponse::NotFound().json(msg),
            _ => HttpResponse::InternalServerError().json(msg),
        }
        .into()
    }
}

impl ResponseError for AgreementError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            AgreementError::NotFound(_) => HttpResponse::NotFound().json(msg).into(),
            AgreementError::Expired(_) => HttpResponse::Gone().json(msg).into(),
            AgreementError::ProposalAlreadyAccepted(..) => {
                HttpResponse::Conflict().json(msg).into()
            }
            AgreementError::UpdateState(_, e) => e.error_response(),
            AgreementError::NoNegotiations(_)
            | AgreementError::ProposalRejected(..)
            | AgreementError::OwnProposal(..)
            | AgreementError::ProposalNotFound(..)
            | AgreementError::ProposalCountered(..)
            | AgreementError::InvalidDate(..)
            | AgreementError::InvalidAgreementState(..)
            | AgreementError::InvalidId(..) => HttpResponse::BadRequest().json(msg).into(),
            AgreementError::GetProposal(..)
            | AgreementError::Save(..)
            | AgreementError::Get(..)
            | AgreementError::Gsb(_)
            | AgreementError::ProtocolCreate(_)
            | AgreementError::Protocol(_)
            | AgreementError::ProtocolTerminate(_)
            | AgreementError::ProtocolCommit(_)
            | AgreementError::Internal(_) => HttpResponse::InternalServerError().json(msg).into(),
        }
    }
}

impl ResponseError for AgreementDaoError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            AgreementDaoError::InvalidTransition { from, .. } => match from {
                AgreementState::Proposal => HttpResponse::Conflict().json(msg),
                AgreementState::Pending
                | AgreementState::Approving
                | AgreementState::Cancelled
                | AgreementState::Rejected
                | AgreementState::Expired
                | AgreementState::Approved
                | AgreementState::Terminated => HttpResponse::Gone().json(msg),
            },
            AgreementDaoError::InvalidId(_) => HttpResponse::BadRequest().json(msg),
            AgreementDaoError::DbError(_)
            | AgreementDaoError::SessionId(_)
            | AgreementDaoError::EventError(_) => HttpResponse::InternalServerError().json(msg),
        }
        .into()
    }
}

impl ResponseError for WaitForApprovalError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            WaitForApprovalError::NotFound(_) => HttpResponse::NotFound().json(msg),
            WaitForApprovalError::Expired(_) => HttpResponse::Gone().json(msg),
            WaitForApprovalError::Terminated(_)
            | WaitForApprovalError::NotConfirmed(_)
            | WaitForApprovalError::InvalidId(..) => HttpResponse::BadRequest().json(msg),
            WaitForApprovalError::Timeout(_) => HttpResponse::RequestTimeout().json(msg),
            WaitForApprovalError::Internal(_) | WaitForApprovalError::Get(..) => {
                HttpResponse::InternalServerError().json(msg)
            }
        }
        .into()
    }
}

impl ResponseError for AgreementEventsError {
    fn error_response(&self) -> HttpResponse {
        let msg = ErrorMessage::new(self.to_string());
        match self {
            AgreementEventsError::InvalidMaxEvents(..) => HttpResponse::BadRequest().json(msg),
            AgreementEventsError::Internal(_) => HttpResponse::InternalServerError().json(msg),
        }
        .into()
    }
}
