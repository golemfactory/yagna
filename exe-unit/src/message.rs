use crate::error::Error;
use crate::Result;
use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ya_model::activity::activity_state::{State, StatePair};
use ya_model::activity::{
    CommandResult, ExeScriptCommand, ExeScriptCommandResult, ExeScriptCommandState,
};

#[derive(Debug, Message)]
#[rtype("()")]
pub struct SetTaskPackagePath(pub PathBuf);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<Vec<f64>>")]
pub struct GetMetrics;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "GetStateResult")]
pub struct GetState;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, MessageResponse)]
pub struct GetStateResult(pub StatePair);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub struct SetState {
    pub state: Option<(StatePair, Option<String>)>,
    pub running_command: Option<Option<ExeScriptCommandState>>,
    pub batch_result: Option<(String, ExeScriptCommandResult)>,
}

impl SetState {
    pub fn exec(state: StatePair, command: ExeScriptCommand) -> Self {
        SetState {
            state: Some((state, None)),
            running_command: Some(Some(command.into())),
            batch_result: None,
        }
    }

    pub fn result(state: StatePair, batch_id: String, result: ExeScriptCommandResult) -> Self {
        SetState {
            state: Some((state, None)),
            running_command: Some(None),
            batch_result: Some((batch_id, result)),
        }
    }
}

impl From<State> for SetState {
    fn from(state: State) -> Self {
        Self::from(StatePair::from(state))
    }
}

impl From<StatePair> for SetState {
    fn from(state: StatePair) -> Self {
        SetState {
            state: Some((state, None)),
            running_command: None,
            batch_result: None,
        }
    }
}

impl From<(State, String)> for SetState {
    fn from(tuple: (State, String)) -> Self {
        SetState {
            state: Some((tuple.0.into(), Some(tuple.1))),
            running_command: None,
            batch_result: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Message)]
#[rtype(result = "Result<ExecCmdResult>")]
pub struct ExecCmd(pub ExeScriptCommand);

#[derive(Clone, Debug)]
pub struct ExecCmdResult {
    pub result: CommandResult,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

impl ExecCmdResult {
    pub fn into_exe_result(self, index: usize) -> ExeScriptCommandResult {
        let message = match self.result {
            CommandResult::Ok => self.stdout,
            CommandResult::Error => self.stderr,
        };
        ExeScriptCommandResult {
            index: index as u32,
            result: Some(self.result),
            message,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Message)]
#[rtype(result = "()")]
pub struct Register<Svc>(pub Addr<Svc>)
where
    Svc: Actor<Context = Context<Svc>> + Handler<Shutdown>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<()>")]
pub struct Shutdown(pub ShutdownReason);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ShutdownReason {
    Finished,
    Interrupted(i32),
    UsageLimitExceeded(String),
    Error(String),
}

impl std::fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            ShutdownReason::Finished => f.write_str("Finished"),
            ShutdownReason::Interrupted(sig) => {
                f.write_str(&format!("Interrupted by signal {}", sig))
            }
            ShutdownReason::UsageLimitExceeded(error) => {
                f.write_str(&format!("Usage limit exceeded: {}", error))
            }
            ShutdownReason::Error(error) => f.write_str(&error),
        }
    }
}

impl From<Error> for ShutdownReason {
    fn from(e: Error) -> Self {
        match e {
            Error::UsageLimitExceeded(reason) => ShutdownReason::UsageLimitExceeded(reason),
            error => ShutdownReason::Error(format!("{:?}", error)),
        }
    }
}

impl Default for ShutdownReason {
    fn default() -> Self {
        ShutdownReason::Finished
    }
}
