use crate::dispatchers::{Dispatcher, GsbDispatcher, InteractiveCli, FileDispatcher};
use crate::supervisor::ExeUnitSupervisorActor;
use crate::exeunit::ExeUnit;

use crate::cmd_args::Config;

use actix::prelude::*;
use anyhow::{Error, Result};
use structopt::StructOpt;


pub struct ExeUnitFramework {
    cmd_input: Box< dyn Dispatcher>,
    supervisor: Addr<ExeUnitSupervisorActor>,
    sys: SystemRunner,
}



impl ExeUnitFramework {
    pub fn new(
        cmd_dispatcher: Box< dyn Dispatcher>,
        exeunit: Box<dyn ExeUnit>
    ) -> Result<ExeUnitFramework> {
        let supervisor = ExeUnitSupervisorActor::new(exeunit).start();
        let mut sys = System::new("ExeUnit");

        Ok(ExeUnitFramework{sys, supervisor, cmd_input: cmd_dispatcher})
    }

    /// Creates ExeUnitFramework using command line args.
    pub fn from_cmd_args(exeunit: Box<dyn ExeUnit>) -> Result<ExeUnitFramework> {

        let dispatcher: Box< dyn Dispatcher>;
        let args = Config::from_args();
        match args {
            Config::CLI => {
                dispatcher = InteractiveCli::new();
            },
            Config::FromFile{ input} => {
                dispatcher = FileDispatcher::new(input);
            },
            Config::Gsb{ service_id } => {
                dispatcher = GsbDispatcher::new(service_id);
            }
        }
        Ok(ExeUnitFramework::new(dispatcher, exeunit)?)
    }

    pub fn run(&mut self) -> Result<()> {
        self.cmd_input.run(self.supervisor.clone(), &mut self.sys);
        Ok(())
    }

}

