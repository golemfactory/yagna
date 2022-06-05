use actix_web::{error::ResponseError, HttpResponse};

use ya_client_model::ErrorMessage;
use ya_core_model::activity::RpcMessageError;
use ya_core_model::market::RpcMessageError as MarketRpcMessageError;

use crate::dao::DaoError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("DAO error: {0}")]
    Dao(#[from] DaoError),
    #[error("GSB error: {0}")]
    Gsb(#[from] ya_service_bus::Error),
    #[error("Service error: {0}")]
    Service(String),
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Timeout")]
    Timeout,
}

impl From<ya_persistence::executor::Error> for Error {
    fn from(e: ya_persistence::executor::Error) -> Self {
        Error::Dao(e.into())
    }
}

impl From<Error> for actix_web::HttpResponse {
    fn from(err: Error) -> Self {
        err.error_response().into()
    }
}

impl From<tokio::time::error::Elapsed> for Error {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        Error::Timeout
    }
}

impl From<RpcMessageError> for Error {
    fn from(e: RpcMessageError) -> Self {
        match e {
            RpcMessageError::Service(msg) => Error::Service(msg),
            RpcMessageError::Activity(msg) => Error::Service(msg),
            RpcMessageError::UsageLimitExceeded(msg) => Error::Service(msg),
            RpcMessageError::BadRequest(msg) => Error::BadRequest(msg),
            RpcMessageError::Forbidden(msg) => Error::Forbidden(msg),
            RpcMessageError::NotFound(msg) => Error::NotFound(msg),
            RpcMessageError::Timeout => Error::Timeout,
        }
    }
}

impl From<MarketRpcMessageError> for Error {
    fn from(e: MarketRpcMessageError) -> Self {
        match e {
            MarketRpcMessageError::Service(msg) => Error::Service(msg),
            MarketRpcMessageError::Market(msg) => Error::Service(msg),
            MarketRpcMessageError::BadRequest(msg) => Error::BadRequest(msg),
            MarketRpcMessageError::Forbidden(msg) => Error::Forbidden(msg),
            MarketRpcMessageError::NotFound(msg) => Error::NotFound(msg),
            MarketRpcMessageError::Timeout => Error::Timeout,
        }
    }
}

impl From<ErrorMessage> for Error {
    fn from(err: ErrorMessage) -> Self {
        Error::BadRequest(err.to_string())
    }
}

impl From<ya_net::NetApiError> for Error {
    fn from(err: ya_net::NetApiError) -> Self {
        Error::BadRequest(err.to_string())
    }
}

impl From<ya_client_model::node_id::ParseError> for Error {
    fn from(err: ya_client_model::node_id::ParseError) -> Self {
        Error::BadRequest(err.to_string())
    }
}

impl From<Error> for RpcMessageError {
    fn from(e: Error) -> Self {
        match e {
            Error::Service(msg) => RpcMessageError::Activity(msg),
            Error::BadRequest(msg) => RpcMessageError::BadRequest(msg),
            Error::NotFound(msg) => RpcMessageError::NotFound(msg),
            Error::Forbidden(msg) => RpcMessageError::Forbidden(msg),
            Error::Timeout => RpcMessageError::Timeout,
            _ => RpcMessageError::Service(e.to_string()),
        }
    }
}

impl ResponseError for Error {
    fn error_response(&self) -> HttpResponse {
        match self {
            Error::BadRequest(_) => {
                HttpResponse::BadRequest().json(ErrorMessage::new(self.to_string()))
            }
            Error::NotFound(_) => {
                HttpResponse::NotFound().json(ErrorMessage::new(self.to_string()))
            }
            Error::Forbidden(_) => {
                HttpResponse::Forbidden().json(ErrorMessage::new(self.to_string()))
            }
            Error::Timeout => {
                HttpResponse::RequestTimeout().json(ErrorMessage::new(self.to_string()))
            }
            _ => {
                let e = self.to_string();
                log::error!("Activity API server error: {}", e);
                HttpResponse::InternalServerError().json(ErrorMessage::new(e))
            }
        }
    }
}
