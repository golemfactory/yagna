use super::dispatcher::Dispatcher;
use crate::supervisor::ExeUnitSupervisor;

use actix::prelude::*;
use anyhow::{Error, Result};


/// Processes commands from command line in interactive mode.
pub struct InteractiveCli {

}

impl InteractiveCli {
    pub fn new() -> Box<dyn Dispatcher> {
        Box::new(InteractiveCli{})
    }
}


impl Dispatcher for InteractiveCli {

    fn run(&mut self, supervisor: Addr<ExeUnitSupervisor>, sys: &mut SystemRunner) -> Result<()> {
        Ok(())
    }
}




