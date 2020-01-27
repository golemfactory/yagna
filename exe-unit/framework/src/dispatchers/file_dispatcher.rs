use super::dispatcher::Dispatcher;
use crate::supervisor::ExeUnitSupervisorActor;

use actix::prelude::*;
use anyhow::{Error, Result};
use std::path::PathBuf;


/// Reads commands from json file and sends them to ExeUnit.
pub struct FileDispatcher {
    file: PathBuf
}


impl FileDispatcher {

    pub fn new(file: PathBuf) -> Box<dyn Dispatcher> {
        Box::new(FileDispatcher{file})
    }
}

impl Dispatcher for FileDispatcher {

    fn run(&mut self, supervisor: Addr<ExeUnitSupervisorActor>, sys: &mut SystemRunner) -> Result<()> {
        Ok(())
    }
}

