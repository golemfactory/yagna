use actix_web::{HttpResponse, Scope};
use awc::http::StatusCode;
use futures::lock::Mutex;
use jsonwebtoken::{encode, Header};
use serde::{Deserialize, Serialize};
use std::collections::{
    hash_map::Entry::{Occupied, Vacant},
    HashMap,
};
use std::time::Duration;

use ya_client::{
    error::Error,
    market::{MarketProviderApi, MarketRequestorApi},
    web::{WebAuth, WebClient, WebInterface},
    Result,
};
use ya_core_model::ethaddr::NodeId;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::scope::ExtendableScope;

use crate::utils::response;

mod provider;
mod requestor;

#[derive(Default)]
struct ClientCache {
    clients: Mutex<HashMap<NodeId, WebClient>>,
    providers: Mutex<HashMap<NodeId, MarketProviderApi>>,
    requestors: Mutex<HashMap<NodeId, MarketRequestorApi>>,
}

impl ClientCache {
    async fn get_api<T: WebInterface>(&self, node_id: NodeId) -> T {
        let mut clients = self.clients.lock().await;
        log::warn!("clients: {}", clients.len());
        clients
            .entry(node_id)
            .or_insert_with(|| build_web_client(node_id))
            .interface()
            .unwrap()
    }

    //TODO: make it return reference to api, to not clone it all the time
    async fn get_privider_api(&self, node_id: NodeId) -> MarketProviderApi {
        let mut providers = self.providers.lock().await;
        match providers.entry(node_id) {
            Occupied(entry) => entry.get().clone(),
            Vacant(entry) => entry.insert(self.get_api(node_id).await).clone(),
        }
    }

    async fn get_requestor_api(&self, node_id: NodeId) -> MarketRequestorApi {
        let mut requestors = self.requestors.lock().await;
        match requestors.entry(node_id) {
            Occupied(entry) => entry.get().clone(),
            Vacant(entry) => entry.insert(self.get_api(node_id).await).clone(),
        }
    }
}

pub fn web_scope(_db: &DbExecutor) -> Scope {
    let client_cache = ClientCache::default();
    Scope::new(crate::MARKET_API_PATH)
        // .data(db.clone())
        .data(client_cache)
        .extend(requestor::extend_web_scope)
        .extend(provider::extend_web_scope)
}

pub const DEFAULT_EVENT_TIMEOUT: f32 = 0.0; // seconds
pub const DEFAULT_REQUEST_TIMEOUT: f32 = 12.0;
pub const AWC_CLIENT_TIMEOUT: f32 = 15.0;

/// Our claims struct, it needs to derive `Serialize` and/or `Deserialize`
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    aud: String,
    sub: String,
}

fn encode_jwt(node_id: NodeId) -> String {
    let claims = Claims {
        aud: String::from("GolemNetHub"),
        sub: String::from(serde_json::json!(node_id).as_str().unwrap_or("unknown")),
    };

    encode(&Header::default(), &claims, "secret".as_ref()).unwrap_or(String::from("error"))
}

fn build_web_client(node_id: NodeId) -> WebClient {
    log::warn!("building new web client for: {}", node_id);
    WebClient::builder()
        .auth(WebAuth::Bearer(encode_jwt(node_id)))
        .timeout(Duration::from_secs_f32(AWC_CLIENT_TIMEOUT))
        .build()
        .unwrap() // we want to panic early
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
#[serde(rename_all = "camelCase")]
pub struct QueryTimeout {
    #[serde(default = "default_query_timeout")]
    pub timeout: Option<f32>,
}

#[inline(always)]
pub(crate) fn default_query_timeout() -> Option<f32> {
    Some(DEFAULT_REQUEST_TIMEOUT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryTimeoutCommandIndex {
    pub timeout: Option<f32>,
    pub command_index: Option<usize>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct QueryTimeoutMaxEvents {
    /// number of milliseconds to wait
    #[serde(default = "default_event_timeout")]
    pub timeout: Option<f32>,
    /// maximum count of events to return
    #[serde(default)]
    pub max_events: Option<i32>,
}

#[inline(always)]
pub(crate) fn default_event_timeout() -> Option<f32> {
    Some(DEFAULT_EVENT_TIMEOUT)
}
