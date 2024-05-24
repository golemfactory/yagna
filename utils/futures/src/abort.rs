use futures3::future::{AbortHandle, AbortRegistration};
use std::cmp::Eq;
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Allows storing AbortHandle objects in a container
#[derive(Clone, Debug)]
pub struct Abort {
    id: usize,
    inner: AbortHandle,
}

impl Abort {
    pub fn new_pair() -> (Self, AbortRegistration) {
        let (handle, reg) = AbortHandle::new_pair();
        (Self::from(handle), reg)
    }

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

impl Hash for Abort {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(&self.id.to_be_bytes())
    }
}

impl Eq for Abort {}

impl PartialEq for Abort {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
