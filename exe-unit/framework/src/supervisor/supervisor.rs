use crate::exeunit::ExeUnit;

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
#[rtype(result = "Result<()>")]
pub struct QueryActivityState;

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct QueryActivityUsage;

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct QueryRunningCommand;

#[derive(Message)]
#[rtype(result = "Result<()>")]
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

