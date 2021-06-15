use crate::error::Error;
use crate::runtime::RuntimeMode;
use crate::state::CommandStateRepr;
use crate::Result;
use actix::prelude::*;
use futures::channel::mpsc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ya_client_model::activity::activity_state::{State, StatePair};
use ya_client_model::activity::{ExeScriptCommand, ExeScriptCommandResult, RuntimeEvent};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<Vec<f64>>")]
pub struct GetMetrics;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "GetStateResponse")]
pub struct GetState;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, MessageResponse)]
pub struct GetStateResponse(pub StatePair);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "GetBatchResultsResponse")]
pub struct GetBatchResults(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, MessageResponse)]
pub struct GetBatchResultsResponse(pub Vec<ExeScriptCommandResult>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Option<String>")]
pub struct GetStdOut {
    pub batch_id: String,
    pub idx: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Message)]
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

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<i32>")]
pub struct ExecuteCommand {
    pub batch_id: String,
    pub idx: usize,
    pub command: ExeScriptCommand,
    pub tx: mpsc::Sender<RuntimeEvent>,
}

impl ExecuteCommand {
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
pub struct CommandContext {
    pub batch_id: String,
    pub idx: usize,
    pub tx: mpsc::Sender<RuntimeEvent>,
}

#[derive(Debug, Message)]
#[rtype(result = "()")]
pub struct SetTaskPackagePath(pub Option<PathBuf>);

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct SetRuntimeMode(pub RuntimeMode);

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct Initialize;

#[derive(Clone, Debug, PartialEq, Message)]
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

#[derive(Debug, thiserror::Error)]
pub enum ShutdownReason {
    #[error("Finished")]
    Finished,
    #[error("Interrupted by signal: {0}")]
    Interrupted(i32),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
    #[error("{0}")]
    Error(#[from] Error),
}

impl Default for ShutdownReason {
    fn default() -> Self {
        ShutdownReason::Finished
    }
}
