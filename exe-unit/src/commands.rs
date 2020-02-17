use crate::service::Service;
use crate::Result;
use crate::{metrics, BatchResult};
use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use ya_model::activity::{ExeScriptCommandState, State};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Vec<u8>")]
pub struct Deploy {}

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
#[rtype(result = "()")]
pub enum SetState {
    State(StateExt),
    RunningCommand(Option<ExeScriptCommandState>),
    BatchResult(String, BatchResult),
}

#[derive(Clone, Debug, PartialEq, Message)]
#[rtype(result = "()")]
pub struct RegisterService<S: Service>(pub Addr<S>);

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
#[rtype(result = "MetricReportRes<M>")]
pub struct MetricReportReq<M: metrics::Metric + 'static>(pub PhantomData<M>);

impl<M: metrics::Metric + 'static> MetricReportReq<M> {
    pub fn new() -> Self {
        MetricReportReq(PhantomData)
    }
}

#[derive(Clone, Debug, MessageResponse)]
pub struct MetricReportRes<M: metrics::Metric + 'static>(pub metrics::MetricReport<M>);
