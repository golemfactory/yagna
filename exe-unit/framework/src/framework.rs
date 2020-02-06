use crate::dispatchers::{Dispatcher, GsbDispatcher, InteractiveCli, FileDispatcher};
use crate::supervisor::ExeUnitSupervisor;
use crate::exeunit::ExeUnit;

use crate::cmd_args::Config;

use actix::prelude::*;
use anyhow::{Error, Result};
use log::info;
use structopt::StructOpt;


pub struct ExeUnitFramework {
    cmd_input: Box< dyn Dispatcher>,
    supervisor: Addr<ExeUnitSupervisor>,
    sys: SystemRunner,
}



impl ExeUnitFramework {
    pub fn new(
        cmd_dispatcher: Box< dyn Dispatcher>,
        exeunit: Box<dyn ExeUnit>
    ) -> Result<ExeUnitFramework> {
        info!("Starting ExeUnit.");

        let mut sys = System::new("ExeUnit");
        let supervisor = ExeUnitSupervisor::new(exeunit).start();

        Ok(ExeUnitFramework{sys, supervisor, cmd_input: cmd_dispatcher})
    }

    /// Creates ExeUnitFramework using command line args.
    pub fn from_cmd_args(exeunit: Box<dyn ExeUnit>) -> Result<ExeUnitFramework> {

        let dispatcher: Box< dyn Dispatcher>;
        let args = Config::from_args();
        match args {
            Config::CLI => {
                dispatcher = InteractiveCli::new();
                info!("Running in interactive CLI mode.");
            },
            Config::FromFile{ input} => {
                dispatcher = FileDispatcher::new(input);
                info!("Running in file commands mode.");
            },
            Config::Gsb{ service_id } => {
                dispatcher = GsbDispatcher::new(service_id);
                info!("Running in gsb dispatcher mode.");
            }
        }
        Ok(ExeUnitFramework::new(dispatcher, exeunit)?)
    }

    pub fn run(self) -> Result<()> {
        let mut cmd = self.cmd_input;
        cmd.run(self.supervisor.clone(), self.sys)
    }

}

