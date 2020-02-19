use crate::error::Error;
use crate::Result;
use actix::prelude::*;
use serde::{Deserialize, Serialize};
use ya_model::activity::activity_state::{State, StatePair};
use ya_model::activity::{ExeScriptCommand, ExeScriptCommandResult, ExeScriptCommandState};

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
    pub state: Option<StatePair>,
    pub running_command: Option<Option<ExeScriptCommandState>>,
    pub batch_result: Option<(String, ExeScriptCommandResult)>,
}

impl From<StatePair> for SetState {
    fn from(state: StatePair) -> Self {
        SetState {
            state: Some(state),
            running_command: None,
            batch_result: None,
        }
    }
}

impl From<State> for SetState {
    fn from(state: State) -> Self {
        SetState {
            state: Some(StatePair::from(state)),
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
    pub result: ExeScriptCommandResult,
    pub stdout: String,
    pub stderr: String,
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
