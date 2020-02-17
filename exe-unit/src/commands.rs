use crate::error::Error;
use crate::{BatchResult, Result};
use actix::prelude::*;
use serde::{Deserialize, Serialize};
use ya_model::activity::{ExeScriptCommandState, State};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StateExt {
    State(State),
    Transitioning { from: State, to: State },
}

impl StateExt {
    pub fn alive(&self) -> bool {
        match &self {
            StateExt::State(state) => match state {
                State::Terminated => false,
                _ => true,
            },
            StateExt::Transitioning {
                from: _,
                to: State::Terminated,
            } => false,
            StateExt::Transitioning { .. } => true,
        }
    }

    pub fn terminated(&self) -> bool {
        match &self {
            StateExt::State(state) => match state {
                State::Terminated => true,
                _ => false,
            },
            _ => false,
        }
    }

    pub fn unwrap(&self) -> State {
        match &self {
            StateExt::State(state) => state.clone(),
            StateExt::Transitioning { from, .. } => from.clone(),
        }
    }
}

impl Default for StateExt {
    fn default() -> Self {
        StateExt::State(State::default())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub enum SetState {
    State(StateExt),
    RunningCommand(Option<ExeScriptCommandState>),
    BatchResult(String, BatchResult),
}

#[derive(Clone, Debug, PartialEq, Message)]
#[rtype(result = "()")]
pub struct RegisterService<Svc>(pub Addr<Svc>)
where
    Svc: Actor<Context = Context<Svc>> + Handler<Shutdown>;

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<()>")]
pub struct Shutdown(pub ShutdownReason);

impl Default for Shutdown {
    fn default() -> Self {
        Shutdown(ShutdownReason::default())
    }
}

unsafe impl Send for Shutdown {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<Vec<f64>>")]
pub struct MetricsRequest;
