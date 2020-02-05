use crate::supervisor::ExeUnitSupervisor;

use actix::prelude::*;
use anyhow::{Error, Result};


/// Dispatchers handle commands input to ExeUnit.
/// It could be gsb, interactive command line or file
/// with commands.
pub trait Dispatcher {
    fn run(&mut self, supervisor: Addr<ExeUnitSupervisor>, sys: &mut SystemRunner) -> Result<()>;
}

