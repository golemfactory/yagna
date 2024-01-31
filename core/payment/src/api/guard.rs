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

#[cfg(test)]
mod tests {
    use super::AgreementLock;
    use std::{
        sync::{
            atomic::{AtomicU32, Ordering},
            Arc,
        },
        time::Duration,
    };

    #[tokio::test]
    async fn take_one() {
        let locks = AgreementLock::arc();

        let _guard = locks.lock("foo".into()).await;
    }

    #[tokio::test]
    async fn take_two() {
        let locks = AgreementLock::arc();

        let _guard1 = locks.lock("foo".into()).await;
        let _guard2 = locks.lock("bar".into()).await;
    }

    #[tokio::test]
    async fn take_one_twice() {
        let locks = AgreementLock::arc();
        let locks_ = Arc::clone(&locks);

        let state = Arc::new(AtomicU32::new(0));
        let state_ = Arc::clone(&state);

        tokio::spawn(async move {
            let guard = locks_.lock("foo".into()).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
            state_.store(1, Ordering::SeqCst);
            drop(guard);

            tokio::time::sleep(Duration::from_millis(500)).await;
            state_.store(2, Ordering::SeqCst);
        });

        tokio::time::sleep(Duration::from_millis(250)).await;
        let _guard = locks.lock("foo".into()).await;

        // state=1 corresponds to the lock being taken in the task above,
        // and released when it is dropped.
        assert_eq!(state.load(Ordering::SeqCst), 1);
    }
}
