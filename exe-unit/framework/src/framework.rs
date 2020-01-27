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
        unimplemented!();
    }

    /// Creates ExeUnitFramework using command line args.
    pub fn from_cmd_args(exeunit: Box<dyn ExeUnit>) -> Result<ExeUnitFramework> {
        let args = Config::from_args();
//        match args {
//            Config::CLI => {
//
//            },
//            Config::FromFile => {
//
//            },
//            Config::Gsb => {
//
//            }
//        }
        Ok(ExeUnitFramework::new(FileDispatcher::new(), exeunit)?)
    }

    pub fn run(&mut self) -> Result<()> {
        unimplemented!();
    }

}

