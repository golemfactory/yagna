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

struct ExeUnitSupervisor {
    exeunit: Box< dyn ExeUnit>
}


// =========================================== //
// Actix stuff
// =========================================== //

/// Wrapper for ExeUnitSupervisor. It is neccesary to use self in async futures.
pub struct ExeUnitSupervisorActor {
    market: Rc<RefCell<ExeUnitSupervisor>>,
}
