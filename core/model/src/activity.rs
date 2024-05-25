//! Activity service bus API.
//!
//! Top level objects constitutes public activity API.
//! Local and Exeunit are in dedicated submodules.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use ya_client_model::activity::{
    ActivityState, ActivityUsage, ExeScriptCommand, ExeScriptCommandResult, ExeScriptCommandState,
    RuntimeEvent,
};
use ya_client_model::NodeId;
use ya_service_bus::{RpcMessage, RpcStreamMessage};

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

    /// Public network VPN bus address for given `network_id`.
    pub fn network_id(network_id: &str) -> String {
        format!("/public/vpn/{}", network_id)
    }
}

// --------

/// Create activity. Returns `activity_id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Create {
    pub provider_id: NodeId,
    pub agreement_id: String,
    pub timeout: Option<f32>,
    // secp256k1 - public key
    #[serde(default)]
    pub requestor_pub_key: Option<Vec<u8>>,
}

impl RpcMessage for Create {
    const ID: &'static str = "CreateActivity";
    type Item = CreateResponseCompat;
    type Error = RpcMessageError;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateResponse {
    pub activity_id: String,
    pub credentials: Option<local::Credentials>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CreateResponseCompat {
    ActivityId(String),
    Response(CreateResponse),
}

impl CreateResponseCompat {
    pub fn activity_id(&self) -> &str {
        match self {
            Self::ActivityId(v) => v.as_ref(),
            Self::Response(r) => r.activity_id.as_ref(),
        }
    }

    pub fn credentials(&self) -> Option<&local::Credentials> {
        match self {
            Self::ActivityId(_) => None,
            Self::Response(r) => r.credentials.as_ref(),
        }
    }
}

impl From<CreateResponseCompat> for CreateResponse {
    fn from(compat: CreateResponseCompat) -> Self {
        match compat {
            CreateResponseCompat::ActivityId(activity_id) => CreateResponse {
                activity_id,
                credentials: None,
            },
            CreateResponseCompat::Response(response) => response,
        }
    }
}

impl From<CreateResponse> for CreateResponseCompat {
    fn from(response: CreateResponse) -> Self {
        CreateResponseCompat::Response(response)
    }
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

/// Update remote network configuration
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VpnControl {
    AddNodes {
        network_id: String,
        nodes: HashMap<String, String>, // IP -> NodeId
    },
    RemoveNodes {
        network_id: String,
        node_ids: HashSet<String>,
    },
}

impl VpnControl {
    pub fn add_node(network_id: String, node_ip: String, node_id: String) -> Self {
        VpnControl::AddNodes {
            network_id,
            nodes: vec![(node_ip, node_id)].into_iter().collect(),
        }
    }

    pub fn remove_node(network_id: String, node_id: String) -> Self {
        VpnControl::RemoveNodes {
            network_id,
            node_ids: vec![(node_id)].into_iter().collect(),
        }
    }
}

impl RpcMessage for VpnControl {
    const ID: &'static str = "VpnControl";
    type Item = ();
    type Error = RpcMessageError;
}

/// Network data
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnPacket(pub Vec<u8>);

impl RpcMessage for VpnPacket {
    const ID: &'static str = "VpnPacket";
    type Item = ();
    type Error = RpcMessageError;
}

pub mod sgx {
    use super::*;

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CallEncryptedService {
        pub activity_id: String,
        pub sender: NodeId,
        pub bytes: Vec<u8>,
    }

    impl RpcMessage for CallEncryptedService {
        const ID: &'static str = "CallEncryptedService";
        type Item = Vec<u8>;
        type Error = RpcMessageError;
    }
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
    pub command_index: Option<usize>,
}

impl RpcMessage for GetExecBatchResults {
    const ID: &'static str = "GetExecBatchResults";
    type Item = Vec<ExeScriptCommandResult>;
    type Error = RpcMessageError;
}

/// Stream script execution events.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamExecBatchResults {
    pub activity_id: String,
    pub batch_id: String,
}

impl RpcStreamMessage for StreamExecBatchResults {
    const ID: &'static str = "StreamExecBatchResults";
    type Item = RuntimeEvent;
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
    type Item = Vec<ExeScriptCommandState>;
    type Error = RpcMessageError;
}

/// Local activity bus API (used by ExeUnit).
///
/// Should be accessible only from local service bus (not via net ie. from remote hosts).
pub mod local {
    use super::*;
    use chrono::{DateTime, Utc};
    use std::collections::BTreeMap;
    use ya_client_model::market::Role;

    /// Local activity bus address.
    pub const BUS_ID: &str = "/local/activity";

    /// Set state of the activity.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Stats {
        pub identity: NodeId,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct StatsResult {
        pub total: BTreeMap<String, u64>,
        pub last_1h: BTreeMap<String, u64>,
        pub last_activity_ts: Option<DateTime<Utc>>,
    }

    impl RpcMessage for Stats {
        const ID: &'static str = "Stats";
        type Item = StatsResult;
        type Error = RpcMessageError;
    }

    /// Set state of the activity.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SetState {
        pub activity_id: String,
        pub state: ActivityState,
        pub timeout: Option<f32>,
        #[serde(default)]
        pub credentials: Option<Credentials>,
    }

    impl SetState {
        pub fn new(
            activity_id: String,
            state: ActivityState,
            credentials: Option<Credentials>,
        ) -> Self {
            SetState {
                activity_id,
                state,
                timeout: Default::default(),
                credentials,
            }
        }
    }

    impl RpcMessage for SetState {
        const ID: &'static str = "SetActivityState";
        type Item = ();
        type Error = RpcMessageError;
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub enum Credentials {
        Sgx {
            requestor: Vec<u8>,
            enclave: Vec<u8>,
            // sha3-256
            payload_sha3: [u8; 32],
            enclave_hash: [u8; 32],
            ias_report: String,
            ias_sig: Vec<u8>,
        },
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

    /// Get agreement ID of the activity.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetAgreementId {
        pub activity_id: String,
        pub timeout: Option<f32>,
        pub role: Role,
    }

    impl RpcMessage for GetAgreementId {
        const ID: &'static str = "GetAgreementId";
        type Item = String;
        type Error = RpcMessageError;
    }
}

/// Error message for activity service bus API.
#[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcMessageError {
    #[error("Service error: {0}")]
    Service(String),
    #[error("Activity API error: {0}")]
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
