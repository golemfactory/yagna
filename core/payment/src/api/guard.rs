use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

/// Registry of locks for agreements
pub(super) struct AgreementLock {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
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
        let mut map = self.locks.lock().await;
        let lock = map.entry(agreement).or_default();
        let guard = Arc::clone(lock).lock_owned().await;
        drop(map);

        AgreementLockGuard {
            guard: Some(guard),
            lock_map: Arc::clone(self),
        }
    }
}

impl Default for AgreementLock {
    fn default() -> Self {
        AgreementLock {
            locks: Mutex::new(HashMap::new()),
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
        let lock_map = Arc::clone(&self.lock_map);

        tokio::task::spawn(async move {
            let mut map = lock_map.locks.lock().await;
            map.retain(|_agreement, lock| lock.try_lock().is_err());
        });
    }
}
