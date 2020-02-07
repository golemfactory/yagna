use crate::exeunit::{ExeUnitBuilder, Worker};
use super::state::StateMachine;
use super::transfers::Transfers;

use ya_model::activity::*;
use ya_utils_actix::forward_actix_handler;
use ya_utils_actix::actix_handler::{ResultTypeGetter, send_message};

use actix::prelude::*;
use anyhow::{Error, Result};
use log::{error, info};


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
pub struct Supervisor {
    worker: Addr<Worker>,
    transfers: Addr<Transfers>,
    states: StateMachine,
    arbiter: Arbiter,
}


impl Supervisor {

    pub fn new(exeunit: Box<dyn ExeUnitBuilder>) -> Supervisor {
        let state_machine = StateMachine::default();

        let arbiter = Arbiter::new();
        let worker = Worker::start_in_arbiter(&arbiter, move |_| Worker::new(exeunit));
        let transfers = Transfers::new().start();

        Supervisor{arbiter, worker, transfers, states: state_machine}
    }

    fn deploy_command(&self, msg: DeployCommand) -> Result<()> {
        info!("Running Deploy command.");
        send_message(self.worker.clone(), msg);
        Ok(())
    }

    fn start_command(&self, msg: StartCommand) -> Result<()> {
        error!("Running Start command. Not implemented.");
        unimplemented!();
    }

    fn run_command(&self, msg: RunCommand) -> Result<()> {
        error!("Running Run command. Not implemented.");
        unimplemented!();
    }

    fn stop_command(&self, msg: StopCommand) -> Result<()> {
        error!("Running Stop command. Not implemented.");
        unimplemented!();
    }

    fn transfer_command(&self, msg: TransferCommand) -> Result<()> {
        error!("Running Transfer command. Not implemented.");
        unimplemented!();
    }
}


// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for Supervisor {
    type Context = Context<Self>;
}

forward_actix_handler!(Supervisor, DeployCommand, deploy_command);
forward_actix_handler!(Supervisor, StartCommand, start_command);
forward_actix_handler!(Supervisor, StopCommand, stop_command);
forward_actix_handler!(Supervisor, TransferCommand, transfer_command);
forward_actix_handler!(Supervisor, RunCommand, run_command);
