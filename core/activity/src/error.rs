use actix_web::{error::ResponseError, HttpResponse};

use ya_core_model::activity::RpcMessageError;
use ya_core_model::market::RpcMessageError as MarketRpcMessageError;
use ya_model::ErrorMessage;

use crate::dao::DaoError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("DB connection error: {0}")]
    Db(#[from] r2d2::Error),
    #[error("DAO error: {0}")]
    Dao(#[from] DaoError),
    #[error("GSB error: {0}")]
    Gsb(ya_service_bus::error::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
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
    #[error("task: {0}")]
    RuntimeError(#[from] tokio::task::JoinError),
}

impl From<ya_persistence::executor::Error> for Error {
    fn from(e: ya_persistence::executor::Error) -> Self {
        log::trace!("ya_persistence::executor::Error: {:?}", e);
        match e {
            ya_persistence::executor::Error::Diesel(e) => Error::from(DaoError::from(e)),
            ya_persistence::executor::Error::Pool(e) => Error::from(e),
            ya_persistence::executor::Error::RuntimeError(e) => Error::from(e),
        }
    }
}

impl Into<actix_web::HttpResponse> for Error {
    fn into(self) -> actix_web::HttpResponse {
        self.error_response()
    }
}

impl From<ya_service_bus::error::Error> for Error {
    fn from(e: ya_service_bus::error::Error) -> Self {
        log::trace!("ya_service_bus::error::Error: {:?}", e);
        Error::Gsb(e)
    }
}

impl From<tokio::time::Elapsed> for Error {
    fn from(_: tokio::time::Elapsed) -> Self {
        log::trace!("tokio::time::Elapsed");
        Error::Timeout
    }
}

impl From<RpcMessageError> for Error {
    fn from(e: RpcMessageError) -> Self {
        log::trace!("RpcMessageError: {:?}", e);
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
        log::trace!("MarketRpcMessageError: {:?}", e);
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

impl From<ya_core_model::ethaddr::ParseError> for Error {
    fn from(err: ya_core_model::ethaddr::ParseError) -> Self {
        Error::BadRequest(err.to_string())
    }
}

impl From<Error> for RpcMessageError {
    fn from(e: Error) -> Self {
        log::trace!("for RpcMessageError: {:?}", e);
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
        log::trace!("actix_web::error::ResponseError: {:?}", self);
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
            _ => HttpResponse::InternalServerError().json(ErrorMessage::new(self.to_string())),
        }
    }
}
