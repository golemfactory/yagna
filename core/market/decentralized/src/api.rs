pub mod provider;
pub mod requestor;
pub mod response;

use serde::{Deserialize, Serialize};

pub const DEFAULT_ACK_TIMEOUT: u32 = 60; // seconds
pub const DEFAULT_EVENT_TIMEOUT: f32 = 0.0; // seconds
pub const DEFAULT_REQUEST_TIMEOUT: f32 = 12.0;

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
