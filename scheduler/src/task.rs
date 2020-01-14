use std::fmt;

pub struct Task {
    name: String,
    task: Box<dyn FnMut() + Sync + Send>,
}

impl Task {
    pub fn new<F>(name: String, task: F) -> Task
    where
        F: 'static + FnMut() + Sync + Send,
    {
        Task {
            name: name,
            task: Box::new(task),
        }
    }

    pub fn execute(&mut self) {
        (*self.task)();
    }
}

impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Task {{ name: {:?} }}", self.name)
    }
}
