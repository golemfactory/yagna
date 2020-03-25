use actix_web::{error::ResponseError, HttpResponse};

use ya_core_model::{appkey, market::RpcMessageError};
use ya_model::ErrorMessage;

use crate::db::models::ConversionError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("DB connection error: {0}")]
    Db(#[from] r2d2::Error),
    #[error("DAO error: {0}")]
    Dao(#[from] diesel::result::Error),
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
    #[error("ya-client error: {0}")]
    ClientError(#[from] ya_client::error::Error),
    #[error("Agreement conversion error: {0}")]
    ConversionError(#[from] ConversionError),
    #[error("App-key error: {0}")]
    AppKeyError(#[from] appkey::Error),
}

impl From<ya_persistence::executor::Error> for Error {
    fn from(e: ya_persistence::executor::Error) -> Self {
        match e {
            ya_persistence::executor::Error::Diesel(e) => Error::from(e),
            ya_persistence::executor::Error::Pool(e) => Error::from(e),
            ya_persistence::executor::Error::RuntimeError(e) => Error::from(e),
        }
    }
}

impl From<Error> for actix_web::HttpResponse {
    fn from(e: Error) -> Self {
        e.error_response()
    }
}

impl From<ya_service_bus::error::Error> for Error {
    fn from(e: ya_service_bus::error::Error) -> Self {
        Error::Gsb(e)
    }
}

impl From<RpcMessageError> for Error {
    fn from(e: RpcMessageError) -> Self {
        match e {
            RpcMessageError::Service(msg) => Error::Service(msg),
            RpcMessageError::Market(msg) => Error::Service(msg),
            RpcMessageError::BadRequest(msg) => Error::BadRequest(msg),
            RpcMessageError::Forbidden(msg) => Error::Forbidden(msg),
            RpcMessageError::NotFound(msg) => Error::NotFound(msg),
            RpcMessageError::Timeout => Error::Timeout,
        }
    }
}

impl From<Error> for RpcMessageError {
    fn from(e: Error) -> Self {
        match e {
            Error::Service(msg) => RpcMessageError::Market(msg),
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
            _ => HttpResponse::InternalServerError().json(ErrorMessage::new(self.to_string())),
        }
    }
}
