//! Activity service bus API.
//!
//! Top level objects constitutes public activity API.
//! Local and Exeunit are in dedicated submodules.
use serde::{Deserialize, Serialize};

use ya_model::activity::{
    ActivityState, ActivityUsage, ExeScriptCommand, ExeScriptCommandResult, ExeScriptCommandState,
};
use ya_service_bus::RpcMessage;

use crate::ethaddr::NodeId;

/// Public Activity bus address.
///
/// # See also
///  * [`local::BUS_ID`](local/constant.BUS_ID.html)
///  * [`exeunit::bus_id`](exeunit/fn.bus_id.html)
pub const BUS_ID: &str = "/public/activity";

/// Public Exe Unit service bus API.
pub mod exeunit {
    /// Public exeunit bus address for given `activity_id`.
    pub fn bus_id(activity_id: &str) -> String {
        format!("/public/exeunit/{}", activity_id)
    }
}

/// Create activity. Returns `activity_id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Create {
    pub provider_id: NodeId,
    pub agreement_id: String,
    pub timeout: Option<f32>,
}

impl RpcMessage for Create {
    const ID: &'static str = "CreateActivity";
    type Item = String;
    type Error = RpcMessageError;
}

/// Destroy activity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Destroy {
    pub agreement_id: String,
    pub activity_id: String,
    pub timeout: Option<f32>,
}

impl RpcMessage for Destroy {
    const ID: &'static str = "DestroyActivity";
    type Item = ();
    type Error = RpcMessageError;
}

/// Get state of the activity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetState {
    pub activity_id: String,
    pub timeout: Option<f32>,
}

impl RpcMessage for GetState {
    const ID: &'static str = "GetActivityState";
    type Item = ActivityState;
    type Error = RpcMessageError;
}

/// Get the activity usage counters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetUsage {
    pub activity_id: String,
    pub timeout: Option<f32>,
}

impl RpcMessage for GetUsage {
    const ID: &'static str = "GetActivityUsage";
    type Item = ActivityUsage;
    type Error = RpcMessageError;
}

/// Execute a script within the activity. Returns `batch_id`.
///
/// Commands are executed sequentially.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exec {
    pub activity_id: String,
    pub batch_id: String,
    pub exe_script: Vec<ExeScriptCommand>,
    pub timeout: Option<f32>,
}

impl RpcMessage for Exec {
    const ID: &'static str = "Exec";
    type Item = String;
    type Error = RpcMessageError;
}

/// Get script execution results.
///
/// Returns vector of results: one for every **already executed** script command.
/// Results are populated upon consecutive exe script commands finish.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetExecBatchResults {
    pub activity_id: String,
    pub batch_id: String,
    pub timeout: Option<f32>,
}

impl RpcMessage for GetExecBatchResults {
    const ID: &'static str = "GetExecBatchResults";
    type Item = Vec<ExeScriptCommandResult>;
    type Error = RpcMessageError;
}

/// Get currently running command and its state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRunningCommand {
    pub activity_id: String,
    pub timeout: Option<f32>,
}

impl RpcMessage for GetRunningCommand {
    const ID: &'static str = "GetRunningCommand";
    type Item = ExeScriptCommandState;
    type Error = RpcMessageError;
}

/// Local activity bus API (used by ExeUnit).
///
/// Should be accessible only from local service bus (not via net ie. from remote hosts).
pub mod local {
    use super::*;

    /// Local activity bus address.
    pub const BUS_ID: &str = "/local/activity";

    /// Set state of the activity.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SetState {
        pub activity_id: String,
        pub state: ActivityState,
        pub timeout: Option<f32>,
    }

    impl RpcMessage for SetState {
        const ID: &'static str = "SetActivityState";
        type Item = ();
        type Error = RpcMessageError;
    }

    /// Set usage counters for the activity.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SetUsage {
        pub activity_id: String,
        pub usage: ActivityUsage,
        pub timeout: Option<f32>,
    }

    impl RpcMessage for SetUsage {
        const ID: &'static str = "SetActivityUsage";
        type Item = ();
        type Error = RpcMessageError;
    }
}

/// Error message for activity service bus API.
#[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcMessageError {
    #[error("Service error: {0}")]
    Service(String),
    #[error("Market API error: {0}")]
    Activity(String),
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Timeout")]
    Timeout,
}
