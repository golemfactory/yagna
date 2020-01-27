use super::dispatcher::Dispatcher;
use crate::supervisor::ExeUnitSupervisorActor;

use actix::prelude::*;
use anyhow::{Error, Result};


/// Processe commands from gsb.
pub struct GsbDispatcher {
    service_id: String
}

impl GsbDispatcher {
    pub fn new(service_id: String) -> Box<dyn Dispatcher> {
        Box::new(GsbDispatcher{service_id})
    }
}



impl Dispatcher for GsbDispatcher {

    fn run(&mut self, supervisor: Addr<ExeUnitSupervisorActor>, sys: &mut SystemRunner) -> Result<()> {
        Ok(())
    }

}

