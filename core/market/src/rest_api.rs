//! Market REST endpoints.
//!
//! Responsibility of these functions is calling respective functions from
//! within market modules and mapping return values to http responses.
//! No market logic is allowed here.

use actix_web::{error::InternalError, http::StatusCode, web::PathConfig};
use serde::Deserialize;

use ya_client::model::ErrorMessage;

use crate::db::model::{
    AgreementId, AppSessionId, OwnerType, ProposalId, ProposalIdParseError, SubscriptionId,
};
use chrono::{DateTime, Utc};

pub(crate) mod common;
mod error;
pub(crate) mod provider;
pub(crate) mod requestor;

const DEFAULT_EVENT_TIMEOUT: f32 = 0.0; // seconds
const DEFAULT_QUERY_TIMEOUT: f32 = 12.0;

pub fn path_config() -> PathConfig {
    PathConfig::default().error_handler(|err, _req| {
        InternalError::new(
            serde_json::to_string(&ErrorMessage::new(err.to_string())).unwrap(),
            StatusCode::BAD_REQUEST,
        )
        .into()
    })
}

#[derive(Deserialize, Clone)]
pub struct PathAgreement {
    pub agreement_id: String,
}

#[derive(Deserialize)]
pub struct PathSubscription {
    pub subscription_id: SubscriptionId,
}

#[derive(Deserialize)]
pub struct PathSubscriptionProposal {
    pub subscription_id: SubscriptionId,
    pub proposal_id: ProposalId,
}

#[derive(Deserialize)]
pub struct QueryAppSessionId {
    #[serde(rename = "appSessionId")]
    pub app_session_id: AppSessionId,
}

#[derive(Deserialize)]
pub struct QueryTimeout {
    #[serde(rename = "timeout", default = "default_query_timeout")]
    pub timeout: f32,
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
    /// number of seconds to wait
    #[serde(rename = "timeout", default = "default_event_timeout")]
    pub timeout: f32,
    /// maximum count of events to return
    #[serde(rename = "maxEvents")]
    pub max_events: Option<i32>,
}

#[derive(Deserialize, Debug)]
pub struct QueryAgreementEvents {
    /// number of seconds to wait
    #[serde(rename = "timeout", default = "default_event_timeout")]
    pub timeout: f32,
    /// maximum count of events to return
    #[serde(rename = "maxEvents")]
    pub max_events: Option<i32>,
    #[serde(rename = "appSessionId")]
    pub app_session_id: Option<String>,
    #[serde(rename = "afterTimestamp")]
    pub after_timestamp: Option<DateTime<Utc>>,
}

#[inline(always)]
pub(crate) fn default_query_timeout() -> f32 {
    DEFAULT_QUERY_TIMEOUT
}

#[inline(always)]
pub(crate) fn default_event_timeout() -> f32 {
    DEFAULT_EVENT_TIMEOUT
}

impl PathAgreement {
    pub fn to_id(self, owner: OwnerType) -> Result<AgreementId, ProposalIdParseError> {
        AgreementId::from_client(&self.agreement_id, owner)
    }
}
