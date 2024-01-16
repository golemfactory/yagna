use std::collections::HashMap;
use std::sync::Arc;

use std::sync::Mutex as StdMutex;
use tokio::sync::Mutex as TokioMutex;

/// Registry of locks for agreements
pub(super) struct AgreementLock {
    locks: StdMutex<HashMap<String, Arc<TokioMutex<()>>>>,
}

impl AgreementLock {
    /// Construct new instances wrapped in [`Arc`]
    pub fn arc() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Take a lock for a given agreement.
    ///
    /// The entry in the internal registry will be automatically cleaned up.
    pub async fn lock(self: &Arc<Self>, agreement: String) -> AgreementLockGuard {
        let lock = Arc::clone(
            self.locks
                .lock()
                .expect("Failed to acquire lock")
                .entry(agreement)
                .or_default(),
        );
        let guard = lock.lock_owned().await;

        AgreementLockGuard {
            guard: Some(guard),
            lock_map: Arc::clone(self),
        }
    }
}

impl Default for AgreementLock {
    fn default() -> Self {
        AgreementLock {
            locks: StdMutex::new(HashMap::new()),
        }
    }
}

/// Lock guard ensuring unique operation on an agreement.
///
/// For use in REST API only. Motivated by a need to synchronize debit note and
/// invoice acceptances.
pub(super) struct AgreementLockGuard {
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
    lock_map: Arc<AgreementLock>,
}

impl Drop for AgreementLockGuard {
    fn drop(&mut self) {
        drop(self.guard.take());

        self.lock_map
            .locks
            .lock()
            .expect("Failed to acquire lock")
            .retain(|_agreement, lock| lock.try_lock().is_err());
    }
}
