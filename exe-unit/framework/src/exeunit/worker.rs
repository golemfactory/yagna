use ya_utils_actix::forward_actix_handler;
use ya_utils_actix::actix_handler::ResultTypeGetter;

use crate::exeunit::ExeUnit;

use actix::prelude::*;
use anyhow::{Error, Result};
use log::{error};


// =========================================== //
// Public exposed messages
// =========================================== //

/// Supervisor will forward some commands to worker notifying
/// that he finished execution of his part of command and passes
/// further work to Worker.
use crate::supervisor::{
    RunCommand,
    StartCommand,
    StopCommand,
    DeployCommand,
    TransferCommand
};



/// Actor responsible for direct interaction with ExeUnit trait
/// implementation. Runs in different thread to perform heavy computations.
pub struct Worker {
    exeunit: Box<dyn ExeUnit>,
}


impl Worker {
    pub fn new(exeunit: Box<dyn ExeUnit>) -> Worker {
        Worker{exeunit}
    }

    fn deploy_command(&self, msg: DeployCommand) -> Result<()> {
        error!("Worker - Running Deploy command. Not implemented.");
        unimplemented!();
    }

    fn start_command(&self, msg: StartCommand) -> Result<()> {
        error!("Worker - Running Start command. Not implemented.");
        unimplemented!();
    }

    fn run_command(&self, msg: RunCommand) -> Result<()> {
        error!("Worker - Running Run command. Not implemented.");
        unimplemented!();
    }

    fn stop_command(&self, msg: StopCommand) -> Result<()> {
        error!("Worker - Running Stop command. Not implemented.");
        unimplemented!();
    }

    /// We get this command after transfer is finished.
    /// Worker isn't responsible for doing anything with this command.
    /// We can notify ExeUnit about this fact and ExeUnit can react to this.
    fn on_transfer_finished(&self, msg: TransferCommand) -> Result<()> {
        error!("Worker - Running Transfer command. Not implemented.");
        unimplemented!();
    }
}

// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for Worker {
    type Context = Context<Self>;
}

forward_actix_handler!(Worker, DeployCommand, deploy_command);
forward_actix_handler!(Worker, StartCommand, start_command);
forward_actix_handler!(Worker, StopCommand, stop_command);
forward_actix_handler!(Worker, TransferCommand, on_transfer_finished);
forward_actix_handler!(Worker, RunCommand, run_command);

