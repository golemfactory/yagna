use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AllocationReleaseTasks {
    pub tasks: Arc<parking_lot::Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl AllocationReleaseTasks {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        }
    }
}
impl Default for AllocationReleaseTasks {
    fn default() -> Self {
        Self::new()
    }
}

lazy_static! {
    static ref ALLOC: Arc<parking_lot::Mutex<Option<AllocationReleaseTasks>>> =
        Arc::new(parking_lot::Mutex::new(None));
}

pub fn init_allocation_release_tasks(tasks: Option<AllocationReleaseTasks>) {
    let mut alloc = ALLOC.lock();
    if alloc.is_none() {
        if let Some(tasks) = tasks {
            *alloc = Some(tasks);
        } else {
            *alloc = Some(AllocationReleaseTasks::new());
        }
    } else {
        panic!("Allocation release tasks are already initialized");
    }
}

pub(crate) fn get_allocation_release_tasks() -> AllocationReleaseTasks {
    let alloc = ALLOC.lock();
    if let Some(tasks) = &*alloc {
        tasks.clone()
    } else {
        panic!("Call init_allocation_release_tasks function before using payment service");
    }
}
