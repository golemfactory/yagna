use crate::error::Error;
use crate::runtime::RuntimeMode;
use crate::state::CommandStateRepr;
use crate::Result;
use actix::prelude::*;
use futures::channel::mpsc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use ya_client_model::activity;
use ya_client_model::activity::activity_state::{State, StatePair};
use ya_client_model::activity::exe_script_command::Network;
use ya_client_model::activity::runtime_event::DeployProgress;
use ya_client_model::activity::{CommandOutput, ExeScriptCommand, ExeScriptCommandResult};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<Vec<f64>>")]
pub struct GetMetrics;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub struct SetMetric {
    pub name: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Message)]
#[rtype(result = "GetStateResponse")]
pub struct GetState;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, MessageResponse)]
pub struct GetStateResponse(pub StatePair);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Message)]
#[rtype(result = "GetBatchResultsResponse")]
pub struct GetBatchResults {
    pub batch_id: String,
    pub idx: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, MessageResponse)]
pub struct GetBatchResultsResponse(pub Vec<ExeScriptCommandResult>);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Message)]
#[rtype(result = "Option<String>")]
pub struct GetStdOut {
    pub batch_id: String,
    pub idx: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub struct SetState {
    pub state: StatePair,
    pub reason: Option<String>,
}

impl SetState {
    pub fn new(state: StatePair, reason: String) -> Self {
        SetState {
            state,
            reason: Some(reason),
        }
    }
}

impl From<State> for SetState {
    #[inline]
    fn from(state: State) -> Self {
        Self::from(StatePair::from(state))
    }
}

impl From<StatePair> for SetState {
    #[inline]
    fn from(state: StatePair) -> Self {
        SetState {
            state,
            reason: None,
        }
    }
}

#[derive(Clone, Debug, Message, derive_more::Display)]
#[display(fmt = "Command: {:?} (batch = {}[{}])", command, batch_id, idx)]
#[rtype(result = "Result<i32>")]
pub struct ExecuteCommand {
    pub batch_id: String,
    pub idx: usize,
    pub command: ExeScriptCommand,
    pub tx: mpsc::Sender<RuntimeEvent>,
}

impl ExecuteCommand {
    pub fn stateless(&self) -> bool {
        matches!(
            &self.command,
            ExeScriptCommand::Sign { .. } | ExeScriptCommand::Terminate { .. }
        )
    }

    pub fn split(self) -> (ExeScriptCommand, CommandContext) {
        (
            self.command,
            CommandContext {
                batch_id: self.batch_id,
                idx: self.idx,
                tx: self.tx,
            },
        )
    }
}

#[derive(Clone, Debug)]
pub enum RuntimeEvent {
    Process(Box<activity::RuntimeEvent>),
    State {
        name: String,
        value: Option<serde_json::Value>,
    },
    Counter {
        name: String,
        value: f64,
    },
}

impl RuntimeEvent {
    pub fn started(batch_id: String, idx: usize, command: ExeScriptCommand) -> Self {
        let kind = activity::RuntimeEventKind::Started { command };
        Self::Process(Box::new(activity::RuntimeEvent::new(batch_id, idx, kind)))
    }

    pub fn finished(
        batch_id: String,
        idx: usize,
        return_code: i32,
        message: Option<String>,
    ) -> Self {
        let kind = activity::RuntimeEventKind::Finished {
            return_code,
            message,
        };
        Self::Process(Box::new(activity::RuntimeEvent::new(batch_id, idx, kind)))
    }

    pub fn stdout(batch_id: String, idx: usize, out: CommandOutput) -> Self {
        let kind = activity::RuntimeEventKind::StdOut(out);
        let event = activity::RuntimeEvent::new(batch_id, idx, kind);
        Self::Process(Box::new(event))
    }

    pub fn stderr(batch_id: String, idx: usize, out: CommandOutput) -> Self {
        let kind = activity::RuntimeEventKind::StdErr(out);
        let event = activity::RuntimeEvent::new(batch_id, idx, kind);
        Self::Process(Box::new(event))
    }

    pub fn deploy_progress(batch_id: String, idx: usize, progress: DeployProgress) -> Self {
        let kind = activity::RuntimeEventKind::DeployProgressUpdate(progress);
        let event = activity::RuntimeEvent::new(batch_id, idx, kind);
        Self::Process(Box::new(event))
    }
}

#[derive(Clone, Debug)]
pub struct CommandContext {
    pub batch_id: String,
    pub idx: usize,
    pub tx: mpsc::Sender<RuntimeEvent>,
}

#[derive(Clone, Debug, Default, Message)]
#[rtype(result = "Result<()>")]
pub struct UpdateDeployment {
    pub task_package: Option<PathBuf>,
    pub runtime_mode: Option<RuntimeMode>,
    pub networks: Option<Vec<Network>>,
    pub hosts: Option<HashMap<String, String>>,
    pub hostname: Option<String>,
    pub volumes: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
}

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct Initialize;

#[derive(Clone, Debug, PartialEq, Eq, Message)]
#[rtype(result = "()")]
pub struct Register<Svc>(pub Addr<Svc>)
where
    Svc: Actor<Context = Context<Svc>> + Handler<Shutdown>;

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<SignExeScriptResponse>")]
pub struct SignExeScript {
    pub batch_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignExeScriptResponse {
    pub output: String,
    pub sig: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignatureStub {
    pub script: Vec<ExeScriptCommand>,
    pub results: Vec<CommandStateRepr>,
    pub digest: String,
}

#[derive(Clone, Debug, Default, Message)]
#[rtype(result = "Result<()>")]
pub struct Stop {
    pub exclude_batches: Vec<String>,
}

#[derive(Debug, Default, Message)]
#[rtype(result = "Result<()>")]
pub struct Shutdown(pub ShutdownReason);

#[derive(Debug, Default, thiserror::Error)]
pub enum ShutdownReason {
    #[default]
    #[error("Finished")]
    Finished,
    #[error("Interrupted by signal: {0}")]
    Interrupted(i32),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
    #[error("{0}")]
    Error(#[from] Error),
}
