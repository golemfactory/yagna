use serde::{Deserialize, Serialize};
use ya_model::activity::{
    ActivityState, ActivityUsage, ExeScriptCommand, ExeScriptCommandResult, ExeScriptCommandState,
};
use ya_service_bus::RpcMessage;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateActivity {
    pub agreement_id: String,
    pub timeout: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DestroyActivity {
    pub agreement_id: String,
    pub activity_id: String,
    pub timeout: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exec {
    pub activity_id: String,
    pub batch_id: String,
    pub exe_script: Vec<ExeScriptCommand>,
    pub timeout: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetExecBatchResults {
    pub activity_id: String,
    pub batch_id: String,
    pub timeout: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRunningCommand {
    pub activity_id: String,
    pub timeout: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetActivityState {
    pub activity_id: String,
    pub timeout: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetActivityUsage {
    pub activity_id: String,
    pub timeout: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcMessageError {
    Service(String),
    Activity(String),
    BadRequest(String),
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

impl RpcMessage for GetActivityUsage {
    const ID: &'static str = "GetUsage";
    type Item = ActivityUsage;
    type Error = RpcMessageError;
}
