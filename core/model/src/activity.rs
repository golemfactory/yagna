use crate::ethaddr::NodeId;
use serde::{Deserialize, Serialize};
use ya_model::activity::{
    ActivityState, ActivityUsage, ExeScriptCommand, ExeScriptCommandResult, ExeScriptCommandState,
};
use ya_service_bus::RpcMessage;

pub const SERVICE_ID: &str = "/activity";
pub const EXEUNIT_SERVICE_ID: &str = "/exeunit";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateActivity {
    pub provider_id: NodeId,
    pub agreement_id: String,
    pub timeout: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DestroyActivity {
    pub agreement_id: String,
    pub activity_id: String,
    pub timeout: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exec {
    pub activity_id: String,
    pub batch_id: String,
    pub exe_script: Vec<ExeScriptCommand>,
    pub timeout: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetExecBatchResults {
    pub activity_id: String,
    pub batch_id: String,
    pub timeout: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRunningCommand {
    pub activity_id: String,
    pub timeout: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetActivityState {
    pub activity_id: String,
    pub timeout: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActivityState {
    pub activity_id: String,
    pub state: ActivityState,
    pub timeout: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetActivityUsage {
    pub activity_id: String,
    pub timeout: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActivityUsage {
    pub activity_id: String,
    pub usage: ActivityUsage,
    pub timeout: Option<f32>,
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

impl RpcMessage for CreateActivity {
    const ID: &'static str = "CreateActivity";
    type Item = String;
    type Error = RpcMessageError;
}

impl RpcMessage for DestroyActivity {
    const ID: &'static str = "DestroyActivity";
    type Item = ();
    type Error = RpcMessageError;
}

impl RpcMessage for Exec {
    const ID: &'static str = "Exec";
    type Item = String;
    type Error = RpcMessageError;
}

impl RpcMessage for GetExecBatchResults {
    const ID: &'static str = "GetExecBatchResults";
    type Item = Vec<ExeScriptCommandResult>;
    type Error = RpcMessageError;
}

impl RpcMessage for GetRunningCommand {
    const ID: &'static str = "GetRunningCommand";
    type Item = ExeScriptCommandState;
    type Error = RpcMessageError;
}

impl RpcMessage for GetActivityState {
    const ID: &'static str = "GetState";
    type Item = ActivityState;
    type Error = RpcMessageError;
}

impl RpcMessage for SetActivityState {
    const ID: &'static str = "SetState";
    type Item = ();
    type Error = RpcMessageError;
}

impl RpcMessage for GetActivityUsage {
    const ID: &'static str = "GetUsage";
    type Item = ActivityUsage;
    type Error = RpcMessageError;
}

impl RpcMessage for SetActivityUsage {
    const ID: &'static str = "SetUsage";
    type Item = ();
    type Error = RpcMessageError;
}
