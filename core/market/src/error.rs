use actix_web::error::ResponseError;
use thiserror::Error;

use ya_core_model::{appkey, market::RpcMessageError};
use ya_model::ErrorMessage;

use crate::db::models::ConversionError;

#[derive(Error, Debug)]
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
    #[error("Not found")]
    NotFound,
    #[error("Forbidden")]
    Forbidden,
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

macro_rules! service_error {
    ($err:expr) => {
        RpcMessageError::Service(format!("{}", $err))
    };
}

macro_rules! internal_error_http_response {
    ($err:expr) => {
        actix_web::HttpResponse::InternalServerError().json(ErrorMessage::new(format!("{}", $err)))
    };
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

impl Into<actix_web::HttpResponse> for Error {
    fn into(self) -> actix_web::HttpResponse {
        self.error_response()
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
            RpcMessageError::Market(err) => Error::Service(err),
            RpcMessageError::Service(err) => Error::Service(err),
            RpcMessageError::BadRequest(err) => Error::BadRequest(err),
            RpcMessageError::Forbidden => Error::Forbidden,
            RpcMessageError::NotFound => Error::NotFound,
            RpcMessageError::Timeout => Error::Timeout,
        }
    }
}

impl From<Error> for RpcMessageError {
    fn from(e: Error) -> Self {
        match e {
            Error::Db(err) => service_error!(err),
            Error::Dao(err) => service_error!(err),
            Error::Gsb(err) => service_error!(err),
            Error::Serialization(err) => service_error!(err),
            Error::Service(err) => RpcMessageError::Market(err),
            Error::BadRequest(err) => RpcMessageError::BadRequest(err),
            Error::NotFound => RpcMessageError::NotFound,
            Error::Forbidden => RpcMessageError::Forbidden,
            Error::Timeout => RpcMessageError::Timeout,
            Error::RuntimeError(err) => service_error!(err),
            Error::ClientError(err) => service_error!(err),
            Error::ConversionError(err) => service_error!(err),
            Error::AppKeyError(err) => service_error!(err),
        }
    }
}

impl actix_web::error::ResponseError for Error {
    fn error_response(&self) -> actix_web::HttpResponse {
        match self {
            Error::Db(err) => internal_error_http_response!(err),
            Error::Dao(err) => internal_error_http_response!(err),
            Error::Gsb(err) => internal_error_http_response!(err),
            Error::Serialization(err) => internal_error_http_response!(err),
            Error::Service(err) => internal_error_http_response!(err),
            Error::BadRequest(err) => {
                actix_web::HttpResponse::BadRequest().json(ErrorMessage::new(err.clone()))
            }
            Error::NotFound => actix_web::HttpResponse::NotFound().finish(),
            Error::Forbidden => actix_web::HttpResponse::Forbidden()
                .json(ErrorMessage::new("Invalid credentials".into())),
            Error::Timeout => actix_web::HttpResponse::RequestTimeout().finish(),
            Error::RuntimeError(err) => internal_error_http_response!(err),
            Error::ClientError(err) => internal_error_http_response!(err),
            Error::ConversionError(err) => internal_error_http_response!(err),
            Error::AppKeyError(err) => internal_error_http_response!(err),
        }
    }
}
