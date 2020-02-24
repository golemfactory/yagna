use crate::dao::DaoError;
use actix_web::error::ResponseError;
use thiserror::Error;
use ya_core_model::activity::RpcMessageError;
use ya_core_model::market::RpcMessageError as MarketRpcMessageError;
use ya_model::activity::{ErrorMessage, ProblemDetails};

#[derive(Error, Debug)]
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
    #[error("Not found")]
    NotFound,
    #[error("Forbidden")]
    Forbidden,
    #[error("Timeout")]
    Timeout,
    #[error("task: {0}")]
    RuntimeError(#[from] tokio::task::JoinError),
}

macro_rules! service_error {
    ($err:expr) => {
        RpcMessageError::Service(format!("{:?}", $err))
    };
}

macro_rules! internal_error_http_response {
    ($err:expr) => {
        actix_web::HttpResponse::InternalServerError().json(ErrorMessage {
            message: Some(format!("{:?}", $err)),
        })
    };
}

impl From<ya_persistence::executor::Error> for Error {
    fn from(e: ya_persistence::executor::Error) -> Self {
        log::error!("ya_persistence::executor::Error: {}", e);
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
        log::error!("ya_service_bus::error::Error: {}", e);
        Error::Gsb(e)
    }
}

impl From<tokio::time::Elapsed> for Error {
    fn from(_: tokio::time::Elapsed) -> Self {
        log::debug!("tokio::time::Elapsed");
        Error::Timeout
    }
}

impl From<RpcMessageError> for Error {
    fn from(e: RpcMessageError) -> Self {
        log::error!("RpcMessageError: {:?}", e);
        match e {
            RpcMessageError::Activity(err) => Error::Service(err),
            RpcMessageError::Service(err) => Error::Service(err),
            RpcMessageError::BadRequest(err) => Error::BadRequest(err),
            RpcMessageError::Forbidden => Error::Forbidden,
            RpcMessageError::NotFound => Error::NotFound,
            RpcMessageError::Timeout => Error::Timeout,
        }
    }
}

impl From<MarketRpcMessageError> for Error {
    fn from(e: MarketRpcMessageError) -> Self {
        log::error!("MarketRpcMessageError: {:?}", e);
        match e {
            MarketRpcMessageError::Market(err) => Error::Service(err),
            MarketRpcMessageError::Service(err) => Error::Service(err),
            MarketRpcMessageError::BadRequest(err) => Error::BadRequest(err),
            MarketRpcMessageError::Forbidden => Error::Forbidden,
            MarketRpcMessageError::NotFound => Error::NotFound,
            MarketRpcMessageError::Timeout => Error::Timeout,
        }
    }
}

impl From<Error> for RpcMessageError {
    fn from(e: Error) -> Self {
        log::debug!("for RpcMessageError: {}", e);
        match e {
            Error::Db(err) => service_error!(err),
            Error::Dao(err) => service_error!(err),
            Error::Gsb(err) => service_error!(err),
            Error::RuntimeError(err) => service_error!(err),
            Error::Serialization(err) => service_error!(err),
            Error::Service(err) => RpcMessageError::Activity(err),
            Error::BadRequest(err) => RpcMessageError::BadRequest(err),
            Error::Forbidden => RpcMessageError::Forbidden,
            Error::NotFound => RpcMessageError::NotFound,
            Error::Timeout => RpcMessageError::Timeout,
        }
    }
}

impl actix_web::error::ResponseError for Error {
    fn error_response(&self) -> actix_web::HttpResponse {
        log::debug!("actix_web::error::ResponseError: {}", self);
        match self {
            Error::Db(err) => internal_error_http_response!(err),
            Error::Dao(err) => internal_error_http_response!(err),
            Error::Gsb(err) => internal_error_http_response!(err),
            Error::RuntimeError(err) => internal_error_http_response!(err),
            Error::Serialization(err) => internal_error_http_response!(err),
            Error::Service(err) => internal_error_http_response!(err),
            Error::BadRequest(err) => actix_web::HttpResponse::BadRequest()
                .json(ProblemDetails::new("Bad request".to_string(), err.clone())),
            Error::Forbidden => actix_web::HttpResponse::Forbidden().json(ProblemDetails::new(
                "Forbidden".to_string(),
                "Invalid credentials".to_string(),
            )),
            Error::NotFound => actix_web::HttpResponse::NotFound().finish(),
            Error::Timeout => actix_web::HttpResponse::RequestTimeout().finish(),
        }
    }
}
