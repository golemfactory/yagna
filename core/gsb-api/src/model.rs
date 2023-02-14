use crate::{
    services::{BindError, FindError, UnbindError},
    GsbError,
};
use actix::MailboxError;
use actix_http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use serde::{Deserialize, Serialize};
use ya_client_model::ErrorMessage;

#[derive(Deserialize)]
pub(crate) struct ServicePath {
    pub address: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub(crate) struct ServiceRequest {
    pub(crate) listen: ServiceListenRequest,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub(crate) struct ServiceResponse {
    pub(crate) listen: ServiceListenResponse,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub(crate) struct ServiceListenRequest {
    pub(crate) on: String,
    pub(crate) components: Vec<String>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub(crate) struct ServiceListenResponse {
    pub(crate) on: String,
    pub(crate) components: Vec<String>,
    pub(crate) links: ServiceLinks,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub(crate) struct ServiceLinks {
    pub(crate) messages: String,
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum GsbApiError {
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl From<BindError> for GsbApiError {
    fn from(error: BindError) -> Self {
        match error {
            BindError::DuplicatedService(_) => Self::BadRequest(error.to_string()),
            BindError::InvalidService(_) => Self::BadRequest(error.to_string()),
        }
    }
}

impl From<UnbindError> for GsbApiError {
    fn from(error: UnbindError) -> Self {
        match error {
            UnbindError::ServiceNotFound(_) => Self::NotFound(error.to_string()),
            UnbindError::InvalidService(_) => Self::BadRequest(error.to_string()),
            UnbindError::UnbindFailed(_) => Self::InternalError(error.to_string()),
        }
    }
}

impl From<FindError> for GsbApiError {
    fn from(error: FindError) -> Self {
        match error {
            FindError::EmptyAddress => Self::BadRequest(error.to_string()),
            FindError::ServiceNotFound(_) => Self::NotFound(error.to_string()),
        }
    }
}

impl From<GsbError> for GsbApiError {
    fn from(value: GsbError) -> Self {
        GsbApiError::InternalError(format!("GSB error: {value}"))
    }
}

impl From<MailboxError> for GsbApiError {
    fn from(value: MailboxError) -> Self {
        GsbApiError::InternalError(format!("Actix error: {value}"))
    }
}

impl From<serde_json::Error> for GsbApiError {
    fn from(value: serde_json::Error) -> Self {
        GsbApiError::InternalError(format!("Serialization error {value}"))
    }
}

impl From<actix_web::Error> for GsbApiError {
    fn from(value: actix_web::Error) -> Self {
        GsbApiError::InternalError(format!("Actix error: {value}"))
    }
}

impl ResponseError for GsbApiError {
    fn status_code(&self) -> StatusCode {
        match *self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> actix_web::HttpResponse<actix_http::body::BoxBody> {
        match self {
            GsbApiError::BadRequest(message) => {
                HttpResponse::BadRequest().json(ErrorMessage::new(message))
            }
            GsbApiError::NotFound(message) => {
                HttpResponse::NotFound().json(ErrorMessage::new(message))
            }
            GsbApiError::InternalError(message) => {
                HttpResponse::InternalServerError().json(ErrorMessage::new(message))
            }
        }
    }
}
