use std::fmt;

pub struct Task {
    name: String,
    task: Box<dyn FnMut() + Sync + Send>,
}

impl Task {
    pub fn new<T, F>(name: T, task: F) -> Task
    where
        T: Into<String>,
        F: 'static + FnMut() + Sync + Send,
    {
        Task {
            name: name.into(),
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
