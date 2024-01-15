use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

lazy_static::lazy_static! {
    static ref LOCK_MAP: Mutex<HashMap<String, Arc<Mutex<()>>>> = Mutex::new(HashMap::new());
}

pub(super) struct PaymentLockGuard {
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl PaymentLockGuard {
    pub async fn lock(agreement: String) -> Self {
        let mut map = LOCK_MAP.lock().await;
        let lock = map.entry(agreement).or_default();
        let guard = Arc::clone(lock).lock_owned().await;

        Self { guard: Some(guard) }
    }
}

impl Drop for PaymentLockGuard {
    fn drop(&mut self) {
        drop(self.guard.take());

        tokio::task::spawn(async move {
            let mut map = LOCK_MAP.lock().await;
            map.retain(|_agreement, lock| lock.try_lock().is_err());
        });
    }
}
