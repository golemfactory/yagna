use actix_web::ResponseError;
use thiserror::Error;
use ya_core_model::activity::RpcMessageError;
use ya_model::activity::{ErrorMessage, ProblemDetails};

#[derive(Error, Debug)]
pub enum Error {
    #[error("DB connection error: {0}")]
    Db(r2d2::Error),
    #[error("DAO error: {0}")]
    Dao(diesel::result::Error),
    #[error("GSB error: {0}")]
    Gsb(ya_service_bus::error::Error),
    #[error("Serialization error: {0}")]
    Serialization(serde_json::Error),
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

impl Error {
    pub fn convert<E, F>(value: E) -> F
    where
        Error: From<E>,
        Error: Into<F>,
    {
        Into::<F>::into(Self::from(value))
    }
}

impl Into<actix_web::HttpResponse> for Error {
    fn into(self) -> actix_web::HttpResponse {
        self.error_response()
    }
}

impl From<r2d2::Error> for Error {
    fn from(e: r2d2::Error) -> Self {
        Error::Db(e)
    }
}

impl From<diesel::result::Error> for Error {
    fn from(e: diesel::result::Error) -> Self {
        Error::Dao(e)
    }
}

impl From<ya_service_bus::error::Error> for Error {
    fn from(e: ya_service_bus::error::Error) -> Self {
        Error::Gsb(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Serialization(e)
    }
}

impl From<crate::timeout::TimeoutError> for Error {
    fn from(_: crate::timeout::TimeoutError) -> Self {
        Error::Timeout
    }
}

impl From<RpcMessageError> for Error {
    fn from(e: RpcMessageError) -> Self {
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

impl From<Error> for RpcMessageError {
    fn from(e: Error) -> Self {
        match e {
            Error::Db(err) => service_error!(err),
            Error::Dao(err) => service_error!(err),
            Error::Gsb(err) => service_error!(err),
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
        match self {
            Error::Db(err) => internal_error_http_response!(err),
            Error::Dao(err) => internal_error_http_response!(err),
            Error::Gsb(err) => internal_error_http_response!(err),
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
