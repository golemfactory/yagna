use crate::message::*;
use actix::prelude::*;
use ya_runtime_api::deploy::StartMode;

mod event;
pub mod process;

pub trait Runtime:
    Actor<Context = Context<Self>>
    + Handler<Shutdown>
    + Handler<ExecuteCommand>
    + Handler<UpdateDeployment>
{
}

#[derive(Clone, Default, Debug)]
pub enum RuntimeMode {
    #[default]
    ProcessPerCommand,
    Service,
}

impl From<StartMode> for RuntimeMode {
    fn from(mode: StartMode) -> Self {
        match mode {
            StartMode::Empty => RuntimeMode::ProcessPerCommand,
            StartMode::Blocking => RuntimeMode::Service,
        }
    }
}
