use super::exeunits_registry::ExeUnitsRegistry;
use super::task::Task;


pub struct TaskRunner {
    registry: ExeUnitsRegistry,
    tasks: Vec<Task>,
}


impl TaskRunner {

    pub fn new() -> TaskRunner {
        TaskRunner{ registry: ExeUnitsRegistry::new(), tasks: vec![] }
    }

    pub fn wait_activity_for_events() {
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

