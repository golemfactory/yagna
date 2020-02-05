use crate::exeunit::ExeUnit;

use ya_model::activity::*;

use actix::prelude::*;
use anyhow::{Error, Result};


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
    args: Vec<String>,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct RunCommand {
    entrypoint: String,
    args: Vec<String>,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct TransferCommand {
    from: String,
    to: String,
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

/// Performs ExeUnit commands. Spawns real implementation of ExeUnit.
pub struct ExeUnitSupervisor {
    exeunit: Box<dyn ExeUnit>
}


impl ExeUnitSupervisor {

    pub fn new(exeunit: Box<dyn ExeUnit>) -> ExeUnitSupervisor {
        ExeUnitSupervisor{exeunit}
    }
}


// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for ExeUnitSupervisor {
    type Context = Context<Self>;
}

