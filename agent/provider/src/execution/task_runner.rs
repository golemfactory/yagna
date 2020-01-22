use super::exeunits_registry::ExeUnitsRegistry;
use super::task::Task;

use actix::prelude::*;

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

    pub fn wait_for_activity_events() {
        // or maybe provider agent should do this.
        unimplemented!();
    }

    pub fn on_create_activity() {
        unimplemented!();
    }

    pub fn on_destroy_activity() {
        unimplemented!();
    }
}

struct TaskRunnerActor {
    runner: Rc<RefCell<TaskRunner>>,
}

impl Actor for TaskRunnerActor {
    type Context = Context<Self>;
}

