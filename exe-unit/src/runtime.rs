use actix::prelude::*;

use ya_runtime_api::deploy::StartMode;

use crate::message::*;

mod event;
pub mod process;

pub trait Runtime:
    Actor<Context = Context<Self>>
    + Handler<Shutdown>
    + Handler<ExecuteCommand>
    + Handler<UpdateDeployment>
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
