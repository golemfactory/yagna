mod api;
mod services;

use actix_http::StatusCode;
use actix_web::ResponseError;
use serde::{Deserialize, Serialize};
use services::GsbServices;
use std::{
    future::Future,
    pin::Pin,
    sync::{MutexGuard, PoisonError},
};
use thiserror::Error;
use ya_service_api_interfaces::Provider;

pub const GSB_API_PATH: &str = "/gsb-api/v1";

pub struct GsbApiService;

impl GsbApiService {
    pub async fn gsb<Context>(_: Context) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn rest<Context: Provider<Self, ()>>(_ctx: &Context) -> actix_web::Scope {
        api::web_scope()
    }
}

#[derive(Error, Debug)]
enum GsbApiError {
    //TODO add msg
    #[error("Bad request")]
    BadRequest,
    //TODO add msg
    #[error("Internal error")]
    InternalError,
    #[error(transparent)]
    Any(#[from] anyhow::Error),
}

impl From<PoisonError<MutexGuard<'_, GsbServices>>> for GsbApiError {
    fn from(_value: PoisonError<MutexGuard<'_, GsbServices>>) -> Self {
        GsbApiError::InternalError
    }
}

impl From<serde_json::Error> for GsbApiError {
    fn from(_value: serde_json::Error) -> Self {
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

#[derive(Serialize, Deserialize, Debug)]
struct WsRequest {
    id: String,
    component: String,
    payload: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
struct WsResponse {
    id: String,
    payload: Vec<u8>,
}

struct WsResult(Result<WsResponse, anyhow::Error>);

trait WsCall {
    fn call(&self, path: String, request: WsRequest) -> Pin<Box<dyn Future<Output = WsResult>>>;
}

type WS_CALL = Box<dyn WsCall + Sync + Send>;
