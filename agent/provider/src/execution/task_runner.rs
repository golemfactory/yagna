use super::exeunits_registry::ExeUnitsRegistry;
use super::task::Task;
use crate::market::provider_market::AgreementSigned;
use crate::gen_actix_handler_sync;

use actix::prelude::*;

use anyhow::{Error, Result};
use std::cell::RefCell;
use std::rc::Rc;



#[allow(dead_code)]
pub struct TaskRunner {
    registry: ExeUnitsRegistry,
    tasks: Vec<Task>,
}

#[allow(dead_code)]
impl TaskRunner {
    pub fn new() -> TaskRunner {
        TaskRunner {
            registry: ExeUnitsRegistry::new(),
            tasks: vec![],
        }
    }

    pub fn wait_for_events() {
        // or maybe provider agent should do this.
        unimplemented!();
    }

    pub fn on_create_activity() {
        unimplemented!();
    }

    pub fn on_destroy_activity() {
        unimplemented!();
    }

    pub fn on_signed_agreement(&mut self, msg: AgreementSigned) -> Result<()> {
        unimplemented!();
    }
}

// =========================================== //
// Actix stuff
// =========================================== //

pub struct TaskRunnerActor {
    runner: Rc<RefCell<TaskRunner>>,
}

impl Actor for TaskRunnerActor {
    type Context = Context<Self>;
}

impl TaskRunnerActor {
    pub fn new() -> TaskRunnerActor {
        TaskRunnerActor{runner: Rc::new(RefCell::new(TaskRunner::new()))}
    }
}

gen_actix_handler_sync!(TaskRunnerActor, AgreementSigned, on_signed_agreement, runner);
