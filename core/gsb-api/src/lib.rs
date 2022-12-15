use std::sync::{MutexGuard, PoisonError};

use actix_http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use services::GsbServices;
use ya_service_api_interfaces::Provider;
use ya_service_bus::serialization::Config;

use thiserror::Error;

mod api;
mod services;

pub const GSB_API_PATH: &str = "/gsb-api/v1";

pub struct GsbApiService;

impl GsbApiService {
    pub async fn gsb<Context>(_: Context) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn rest<Context: Provider<Self, ()>>(ctx: &Context) -> actix_web::Scope {
        api::web_scope()
    }
}

#[derive(Error, Debug)]
enum GsbApiError {
    #[error("Bad request")]
    BadRequest,
    #[error("Internal error")]
    InternalError,
    #[error(transparent)]
    Any(#[from] anyhow::Error),
}

impl From<PoisonError<MutexGuard<'_, GsbServices>>> for GsbApiError {
    fn from(value: PoisonError<MutexGuard<'_, GsbServices>>) -> Self {
        GsbApiError::InternalError
    }
}

impl ResponseError for GsbApiError {
    fn status_code(&self) -> StatusCode {
        match *self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
