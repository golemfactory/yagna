use actix::prelude::*;
use serde::{Deserialize, Serialize};
use ya_model::activity::State;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Vec<u8>")]
pub struct Deploy(Vec<u8>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Vec<u8>")]
pub struct Start(Vec<u8>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Vec<u8>")]
pub struct Run(Vec<u8>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Vec<u8>")]
pub struct Stop(Vec<u8>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Vec<u8>")]
pub struct Transfer(Vec<u8>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Vec<u8>")]
pub enum RuntimeCommand {
    Deploy(Deploy),
    Start(Start),
    Run(Run),
    Stop(Stop),
    Transfer(Transfer),
}

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
pub struct Batch {
    pub id: String,
    pub commands: Vec<RuntimeCommand>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "StateExt")]
pub struct GetState;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Option<RuntimeCommand>")]
pub struct GetRunningCommand;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Vec<Vec<u8>>")]
pub struct GetBatchResults {
    pub batch_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub enum SetState {
    State(StateExt),
    RunningCommand(Option<RuntimeCommand>),
    BatchResult(String, Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<(), LocalError>")]
pub struct Shutdown;

unsafe impl Send for Shutdown {}

impl Shutdown {
    pub fn new() -> Self {
        Shutdown {}
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub struct Signal(pub i32);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LocalError {
    InvalidStateError,
    UnsupportedSignalError,
}
