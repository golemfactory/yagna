use crate::error::Error;
use crate::runtime::RuntimeMode;
use crate::Result;
use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ya_client_model::activity::activity_state::{State, StatePair};
use ya_client_model::activity::{
    CommandResult, ExeScriptCommand, ExeScriptCommandResult, ExeScriptCommandState,
};
use ya_runtime_api::server::proto;

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

#[derive(Debug, Message)]
#[rtype(result = "()")]
pub struct SetTaskPackagePath(pub PathBuf);

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct SetRuntimeMode(pub RuntimeMode);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub struct SetState {
    pub state: Option<StateUpdate>,
    pub running_command: Option<CommandUpdate>,
    pub batch_result: Option<ResultUpdate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StateUpdate {
    pub state: StatePair,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandUpdate {
    pub cmd: Option<ExeScriptCommandState>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultUpdate {
    pub batch_id: String,
    pub result: ExeScriptCommandResult,
}

impl SetState {
    pub fn state(mut self, state: StatePair) -> Self {
        self.state = Some(StateUpdate {
            state,
            reason: None,
        });
        self
    }

    pub fn state_reason(mut self, state: StatePair, reason: String) -> Self {
        self.state = Some(StateUpdate {
            state,
            reason: Some(reason),
        });
        self
    }

    pub fn cmd(mut self, command: Option<ExeScriptCommand>) -> Self {
        self.running_command = Some(CommandUpdate {
            cmd: command.map(|c| c.into()),
        });
        self
    }

    pub fn result(mut self, batch_id: String, result: ExeScriptCommandResult) -> Self {
        self.batch_result = Some(ResultUpdate { batch_id, result });
        self
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
        Self::default().state(state)
    }
}

#[derive(Clone, Debug, PartialEq, Message)]
#[rtype(result = "Result<RuntimeCommandResult>")]
pub struct RuntimeCommand(pub ExeScriptCommand);

#[derive(Clone, Debug)]
pub struct RuntimeCommandResult {
    pub result: CommandResult,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

impl RuntimeCommandResult {
    pub fn ok() -> Self {
        RuntimeCommandResult {
            result: CommandResult::Ok,
            stdout: None,
            stderr: None,
        }
    }

    pub fn error(err: impl ToString) -> Self {
        RuntimeCommandResult {
            result: CommandResult::Error,
            stdout: None,
            stderr: Some(err.to_string()),
        }
    }
}

impl From<proto::response::Error> for RuntimeCommandResult {
    fn from(err_resp: proto::response::Error) -> Self {
        let result = match err_resp.code {
            0 => CommandResult::Ok,
            _ => CommandResult::Error,
        };
        let mut stderr = err_resp.message;
        for (key, value) in err_resp.context {
            stderr = format!("{}\n{} = {}", stderr, key, value);
        }

        RuntimeCommandResult {
            result,
            stdout: None,
            stderr: Some(stderr),
        }
    }
}

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct Initialize;

#[derive(Clone, Debug, PartialEq, Message)]
#[rtype(result = "()")]
pub struct Register<Svc>(pub Addr<Svc>)
where
    Svc: Actor<Context = Context<Svc>> + Handler<Shutdown>;

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct Stop {
    pub exclude_batches: Vec<String>,
}

impl Default for Stop {
    fn default() -> Self {
        Stop {
            exclude_batches: Vec::new(),
        }
    }
}

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
