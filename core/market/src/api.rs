use crate::utils::response;
use actix_web::{HttpResponse, Scope};
use awc::http::StatusCode;
use jsonwebtoken::{encode, Header};
use serde::{Deserialize, Serialize};
use ya_client::error::Error;
use ya_client_model::market::MARKET_API_PATH;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_api_web::scope::ExtendableScope;

mod provider;
mod requestor;

pub fn web_scope(db: &DbExecutor) -> Scope {
    Scope::new(MARKET_API_PATH)
        .data(db.clone())
        .extend(requestor::extend_web_scope)
        .extend(provider::extend_web_scope)
}

pub const DEFAULT_ACK_TIMEOUT: u32 = 60; // seconds
pub const DEFAULT_EVENT_TIMEOUT: f32 = 0.0; // seconds
pub const DEFAULT_REQUEST_TIMEOUT: f32 = 12.0;

/// Our claims struct, it needs to derive `Serialize` and/or `Deserialize`
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    aud: String,
    sub: String,
}

pub(crate) fn encode_jwt(id: Identity) -> String {
    let claims = Claims {
        aud: String::from("GolemNetHub"),
        sub: String::from(serde_json::json!(id.identity).as_str().unwrap_or("unknown")),
    };

    encode(&Header::default(), &claims, "secret".as_ref()).unwrap_or(String::from("error"))
}

pub(crate) fn resolve_web_error(err: Error) -> HttpResponse {
    match err {
        Error::HttpStatusCode {
            code,
            url: _,
            msg,
            bt: _,
        } => match code {
            StatusCode::UNAUTHORIZED => response::unauthorized(),
            StatusCode::NOT_FOUND => response::not_found(),
            StatusCode::CONFLICT => response::conflict(),
            StatusCode::GONE => response::gone(),
            _ => response::server_error(&msg),
        },
        _ => response::server_error(&err),
    }
}

#[derive(Deserialize)]
pub struct PathAgreement {
    pub agreement_id: String,
}

#[derive(Deserialize)]
pub struct PathSubscription {
    pub subscription_id: String,
}

#[derive(Deserialize)]
pub struct PathSubscriptionProposal {
    pub subscription_id: String,
    pub proposal_id: String,
}

#[derive(Deserialize)]
pub struct QueryTimeout {
    #[serde(rename = "timeout", default = "default_query_timeout")]
    pub timeout: Option<f32>,
}

#[derive(Deserialize)]
pub struct QueryTimeoutCommandIndex {
    #[serde(rename = "timeout")]
    pub timeout: Option<f32>,
    #[serde(rename = "commandIndex")]
    pub command_index: Option<usize>,
}

#[derive(Deserialize, Debug)]
pub struct QueryTimeoutMaxEvents {
    /// number of milliseconds to wait
    #[serde(rename = "timeout", default = "default_event_timeout")]
    pub timeout: Option<f32>,
    /// maximum count of events to return
    #[serde(rename = "maxEvents", default)]
    pub max_events: Option<i32>,
}

#[inline(always)]
pub(crate) fn default_query_timeout() -> Option<f32> {
    Some(DEFAULT_REQUEST_TIMEOUT)
}

#[inline(always)]
pub(crate) fn default_ack_timeout() -> u32 {
    DEFAULT_ACK_TIMEOUT
}

#[inline(always)]
pub(crate) fn default_event_timeout() -> Option<f32> {
    Some(DEFAULT_EVENT_TIMEOUT)
}

#[derive(Deserialize)]
pub struct Timeout {
    #[serde(default = "default_ack_timeout")]
    pub timeout: u32,
}
