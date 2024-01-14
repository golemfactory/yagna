use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

lazy_static::lazy_static! {
    static ref PAYMENT_LOCK: Mutex<HashMap<String, Arc<Mutex<()>>>> = Mutex::new(HashMap::new());
}

pub(super) struct PaymentLockGuard {
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl PaymentLockGuard {
    pub async fn lock(agreement: String) -> Self {
        let mut map = PAYMENT_LOCK.lock().await;
        let lock = map.entry(agreement).or_default();
        let guard = Arc::clone(lock).lock_owned().await;

        Self { guard: Some(guard) }
    }
}

impl Drop for PaymentLockGuard {
    fn drop(&mut self) {
        drop(self.guard.take());

        tokio::task::spawn(async move {
            let mut map = PAYMENT_LOCK.lock().await;
            map.retain(|_activity, lock| lock.try_lock().is_err());
        });
    }
}
