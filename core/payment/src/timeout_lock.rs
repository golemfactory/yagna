use std::time::Duration;

use futures::Future;
use tokio::{
    sync::{Mutex, MutexGuard},
    time::Timeout,
};

pub trait MutexTimeoutExt<T: ?Sized + 'static> {
    fn timeout_lock(&self, duration: Duration) -> Timeout<impl Future<Output = MutexGuard<'_, T>>>;
}

impl<T: ?Sized + 'static> MutexTimeoutExt<T> for Mutex<T> {
    fn timeout_lock(&self, duration: Duration) -> Timeout<impl Future<Output = MutexGuard<'_, T>>> {
        tokio::time::timeout(duration, self.lock())
    }
}
