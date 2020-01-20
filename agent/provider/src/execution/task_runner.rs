use exeunits_registry::ExeUnitsRegistry;
use task::Task;


pub struct TaskRunner {
    registry: ExeUnitsRegistry,
    tasks: Vec<Task>,
}


impl TaskRunner {

    pub fn new() -> TaskRunner {
        TaskRunner{ registry: ExeUnitsRegistry, tasks: vec![] }
    }

    pub fn wait_activity_for_events() {
        unimplemented!();
    }

    pub fn on_create_activity() {
        unimplemented!();
    }

    pub fn on_destroy_activity() {
        unimplemented!();
    }
}

