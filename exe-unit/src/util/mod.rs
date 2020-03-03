use futures::future::AbortHandle;
use std::sync::atomic::{AtomicUsize, Ordering};

pub mod path;
pub mod url;

/// Allows storing of AbortHandle objects in a Vec
#[derive(Clone, Debug)]
pub struct Abort {
    id: usize,
    inner: AbortHandle,
}

impl Abort {
    pub fn abort(&self) {
        self.inner.abort()
    }
}

impl From<AbortHandle> for Abort {
    fn from(h: AbortHandle) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(1);
        Abort {
            id: COUNTER.fetch_add(1, Ordering::SeqCst),
            inner: h,
        }
    }
}

impl PartialEq for Abort {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
