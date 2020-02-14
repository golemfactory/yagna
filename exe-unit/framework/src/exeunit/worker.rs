use ya_utils_actix::forward_actix_handler;
use ya_utils_actix::actix_handler::ResultTypeGetter;

use crate::exeunit::{ExeUnitBuilder, ExeUnit, DirectoryMount};

use actix::prelude::*;
use anyhow::{Error, Result};
use log::{error, info};
use std::env;


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
use std::fs::DirEntry;
use std::path::PathBuf;


/// Actor responsible for direct interaction with ExeUnit trait
/// implementation. Runs in different thread to perform heavy computations.
pub struct Worker {
    exeunit_factory: Box<dyn ExeUnitBuilder>,
    exeunit: Option<Box<dyn ExeUnit>>,
}


impl Worker {
    pub fn new(exeunit_factory: Box<dyn ExeUnitBuilder>) -> Worker {
        Worker{exeunit_factory, exeunit: None}
    }

    fn create_mount_points() -> Result<Vec<DirectoryMount>> {
        let path = env::current_dir()?;
        let mut mounts = vec![];

        let input = path.join("input");
        let output = path.join("output");

        mounts.push(DirectoryMount{host: input, guest: PathBuf::from("in")});
        mounts.push(DirectoryMount{host: output, guest: PathBuf::from("out")});
        Ok(mounts)
    }

    fn deploy_command(&mut self, msg: DeployCommand) -> Result<()> {
        info!("Worker - Running Deploy command.");

        if let None = self.exeunit {
            let mounts = Worker::create_mount_points()?;
            let mut exeunit = self.exeunit_factory.create(mounts)?;
            self.exeunit = Some(exeunit);
        }

        // Unwrap because we created exeunit in lines above.
        self.exeunit.as_mut().unwrap().on_deploy(msg.args)?;

        info!("Worker - Deploying finished.");
        Ok(())
    }

    fn start_command(&self, msg: StartCommand) -> Result<()> {
        error!("Worker - Running Start command. Not implemented.");
        unimplemented!();
    }

    fn run_command(&mut self, msg: RunCommand) -> Result<()> {
        info!("Worker - Running Run command.");

        if let Some(exeunit) = self.exeunit.as_mut() {
            return Ok(exeunit.on_run(msg.args)?)
        }
        Err(Error::msg(format!("ExeUnit was not deployed.")))
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

