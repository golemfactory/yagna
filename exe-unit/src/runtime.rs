use crate::message::*;
use actix::prelude::*;
use ya_runtime_api::deploy::StartMode;

mod event;
pub mod process;

pub trait Runtime:
    Actor<Context = Context<Self>>
    + Handler<Shutdown>
    + Handler<ExecuteCommand>
    + Handler<SetTaskPackagePath>
    + Handler<SetRuntimeMode>
{
}

#[derive(Clone, Debug)]
pub enum RuntimeMode {
    ProcessPerCommand,
    Service,
}

impl Default for RuntimeMode {
    fn default() -> Self {
        RuntimeMode::ProcessPerCommand
    }
}

impl From<StartMode> for RuntimeMode {
    fn from(mode: StartMode) -> Self {
        match mode {
            StartMode::Empty => RuntimeMode::ProcessPerCommand,
            StartMode::Blocking => RuntimeMode::Service,
        }
    }
}
