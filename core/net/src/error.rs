use actix_web::{ResponseError, HttpResponse};
use ya_client_model::ErrorMessage;

pub type Result<T> = std::result::Result<T, NetError>;

#[derive(thiserror::Error, Debug)]
pub enum NetError {
    //TODO handle more specific cases
    #[error("Some error")]
    AnyError(#[from] anyhow::Error)
}

impl ResponseError for NetError {
    fn error_response(&self) -> HttpResponse {
        match self {
            _ => {
                let e = self.to_string();
                log::error!("Network API server error: {}", e);
                HttpResponse::InternalServerError().json(ErrorMessage::new(e))
            }
        }
    }
}
