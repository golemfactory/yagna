use actix_web::{HttpResponse, ResponseError};
use ya_client_model::ErrorMessage;

pub type Result<T> = std::result::Result<T, NetError>;

#[allow(dead_code)]
#[derive(thiserror::Error, Debug)]
pub enum NetError {
    //TODO handle more specific cases
    #[error("Error: {0}")]
    Error(#[from] anyhow::Error),
    #[error("Bad Request: {0}")]
    BadRequest(String),
}

impl ResponseError for NetError {
    fn error_response(&self) -> HttpResponse {
        match self {
            NetError::Error(_) => {
                log::error!("Network API server error: {}", self);
                HttpResponse::InternalServerError().json(ErrorMessage::new(self))
            }
            NetError::BadRequest(_) => {
                log::error!("Network API server error: {}", self);
                HttpResponse::BadRequest().json(ErrorMessage::new(self))
            }
        }
    }
}
