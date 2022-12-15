use std::{
    future::Future,
    sync::{MutexGuard, PoisonError},
};

use actix_http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use serde::{Deserialize, Serialize};
use services::GsbServices;
use ya_core_model::gftp::{GetChunk, GetMetadata, GftpChunk, GftpMetadata};
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

// impl From<GetMetadata> for WsRequest {
//     fn from(value: GetMetadata) -> Self {
//         todo!()
//     }
// }

// impl From<GftpMetadata> for WsResponse {
//     fn from(value: GftpMetadata) -> Self {
//         todo!()
//     }
// }

// impl From<GetChunk> for WsRequest {
//     fn from(value: GetChunk) -> Self {
//         todo!()
//     }
// }

// impl From<GftpChunk> for WsResponse {
//     fn from(value: GftpChunk) -> Self {
//         todo!()
//     }
// }

type WS_CALL = Box<
    dyn FnMut(String, WsRequest) -> Future<Output = Result<WsResponse, anyhow::Error>>
        + Sync
        + Send,
>;
