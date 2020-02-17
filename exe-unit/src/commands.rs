use crate::BatchResult;
use crate::Result;
use actix::prelude::*;
use serde::{Deserialize, Serialize};
use ya_model::activity::{ExeScriptCommandState, State};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StateExt {
    State(State),
    Transitioning { from: State, to: State },
    ShuttingDown,
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
}

impl Default for ShutdownReason {
    fn default() -> Self {
        ShutdownReason::Finished
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<()>")]
pub struct Shutdown(pub ShutdownReason);

unsafe impl Send for Shutdown {}

impl Default for Shutdown {
    fn default() -> Self {
        Shutdown(ShutdownReason::default())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<Vec<f64>>")]
pub struct MetricsRequest;
