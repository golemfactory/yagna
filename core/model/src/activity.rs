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

pub mod local {
    use super::*;

    pub const BUS_ID: &str = "/local/activity";

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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcMessageError {
    Service(String),
    Activity(String),
    BadRequest(String),
    UsageLimitExceeded(String),
    NotFound,
    Forbidden,
    Timeout,
}
