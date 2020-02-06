use crate::exeunit::ExeUnit;
use super::state::StateMachine;

use ya_model::activity::*;
use ya_utils_actix::forward_actix_handler;
use ya_utils_actix::actix_handler::ResultTypeGetter;

use actix::prelude::*;
use anyhow::{Error, Result};
use log::{error};


// =========================================== //
// Public exposed messages
// =========================================== //

// ====================== //
// ExeUnit commands

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct DeployCommand;

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct StartCommand {
    pub args: Vec<String>,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct RunCommand {
    pub entrypoint: String,
    pub args: Vec<String>,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct TransferCommand {
    pub from: String,
    pub to: String,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct StopCommand;

// ====================== //
// ExeUnit state

#[derive(Message)]
#[rtype(result = "Result<ActivityState>")]
pub struct QueryActivityState;

#[derive(Message)]
#[rtype(result = "Result<ActivityUsage>")]
pub struct QueryActivityUsage;

#[derive(Message)]
#[rtype(result = "Result<ExeScriptCommandState>")]
pub struct QueryRunningCommand;

#[derive(Message)]
#[rtype(result = "Result<Vec<ExeScriptCommandResult>>")]
pub struct QueryExecBatchResults;


// =========================================== //
// ExeUnitSupervisor implementation
// =========================================== //

/// This actor is responsible for performing ExeUnit commands
/// and answering questions about ExeUnit state.
/// It spawns Worker with real implementation of ExeUnit to do the work.
pub struct ExeUnitSupervisor {
    exeunit: Box<dyn ExeUnit>,
    states: StateMachine,
}


impl ExeUnitSupervisor {

    pub fn new(exeunit: Box<dyn ExeUnit>) -> ExeUnitSupervisor {
        let state_machine = StateMachine::default();
        ExeUnitSupervisor{exeunit, states: state_machine}
    }

    fn start_command(&self, msg: StartCommand) -> Result<()> {
        error!("Running Start command. Not implemented.");
        unimplemented!();
    }

    fn stop_command(&self, msg: StopCommand) -> Result<()> {
        error!("Running Stop command. Not implemented.");
        unimplemented!();
    }

    fn deploy_command(&self, msg: DeployCommand) -> Result<()> {
        error!("Running Deploy command. Not implemented.");
        unimplemented!();
    }

    fn transfer_command(&self, msg: TransferCommand) -> Result<()> {
        error!("Running Transfer command. Not implemented.");
        unimplemented!();
    }

    fn run_command(&self, msg: RunCommand) -> Result<()> {
        error!("Running Run command. Not implemented.");
        unimplemented!();
    }
}


// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for ExeUnitSupervisor {
    type Context = Context<Self>;
}

forward_actix_handler!(ExeUnitSupervisor, DeployCommand, deploy_command);
forward_actix_handler!(ExeUnitSupervisor, StartCommand, start_command);
forward_actix_handler!(ExeUnitSupervisor, StopCommand, stop_command);
forward_actix_handler!(ExeUnitSupervisor, TransferCommand, transfer_command);
forward_actix_handler!(ExeUnitSupervisor, RunCommand, run_command);
