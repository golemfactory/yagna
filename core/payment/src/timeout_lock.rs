use std::time::Duration;

use futures::Future;
use tokio::{
    sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard},
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

pub trait RwLockTimeoutExt<T: ?Sized + 'static> {
    fn timeout_read(
        &self,
        duration: Duration,
    ) -> Timeout<impl Future<Output = RwLockReadGuard<'_, T>>>;

    fn timeout_write(
        &self,
        duration: Duration,
    ) -> Timeout<impl Future<Output = RwLockWriteGuard<'_, T>>>;
}

impl<T: ?Sized + 'static> RwLockTimeoutExt<T> for RwLock<T> {
    fn timeout_read(
        &self,
        duration: Duration,
    ) -> Timeout<impl Future<Output = RwLockReadGuard<'_, T>>> {
        tokio::time::timeout(duration, self.read())
    }

    fn timeout_write(
        &self,
        duration: Duration,
    ) -> Timeout<impl Future<Output = RwLockWriteGuard<'_, T>>> {
        tokio::time::timeout(duration, self.write())
    }
}
