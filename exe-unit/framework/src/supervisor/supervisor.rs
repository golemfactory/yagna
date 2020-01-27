use crate::exeunit::ExeUnit;

use actix::prelude::*;

use std::cell::RefCell;
use std::rc::Rc;


// =========================================== //
// Public exposed messages
// =========================================== //



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

/// Wrapper for ExeUnitSupervisor. It is neccesary to use self in async futures.
pub struct ExeUnitSupervisorActor {
    supervisor: Rc<RefCell<ExeUnitSupervisor>>,
}

impl Actor for ExeUnitSupervisorActor {
    type Context = Context<Self>;
}

impl ExeUnitSupervisorActor {

    pub fn new(exeunit: Box<dyn ExeUnit>) -> ExeUnitSupervisorActor {
        let rc = Rc::new(RefCell::new(ExeUnitSupervisor::new(exeunit)));
        ExeUnitSupervisorActor { supervisor: rc }
    }
}
