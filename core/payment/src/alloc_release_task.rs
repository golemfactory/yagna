use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AllocationReleaseTasks {
    pub tasks: Arc<parking_lot::Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl AllocationReleaseTasks {
    fn new_internal() -> Self {
        Self {
            tasks: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        }
    }

    /// Creates a new instance of `AllocationReleaseTasks` for use in tests or mocks.
    pub fn new_for_mocks_only() -> Self {
        Self::new_internal()
    }
}

lazy_static! {
    static ref ALLOC: Arc<parking_lot::Mutex<Option<AllocationReleaseTasks>>> =
        Arc::new(parking_lot::Mutex::new(None));
}

pub fn init_allocation_release_tasks() {
    let mut alloc = ALLOC.lock();
    if alloc.is_none() {
        *alloc = Some(AllocationReleaseTasks::new_internal());
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
