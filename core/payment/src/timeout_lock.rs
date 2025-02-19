use std::time::Duration;

use futures::{Future, FutureExt};
use tokio::time::error::Elapsed;
use tokio::time::Instant;
use tokio::{
    sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::Timeout,
};

pub trait MutexTimeoutExt<T: ?Sized + 'static> {
    fn timeout_lock(
        &self,
        duration: Duration,
    ) -> impl Future<Output = Result<MutexGuard<'_, T>, Elapsed>>;
}
static ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
impl<T: ?Sized + 'static> MutexTimeoutExt<T> for Mutex<T> {
    #[track_caller]
    fn timeout_lock(
        &self,
        duration: Duration,
    ) -> impl Future<Output = Result<MutexGuard<'_, T>, Elapsed>> {
        let caller_location = std::panic::Location::caller();
        let caller_line_number = caller_location.line();
        let caller_file = caller_location.file();

        let next_id = ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let caller = format!("{}:{}", caller_file, caller_line_number);
        log::trace!("Timeout lock {next_id} requested from {caller}");
        let curr = Instant::now();
        tokio::time::timeout(duration, self.lock()).then(move |result| {
            let elapsed_ms = curr.elapsed().as_secs_f64() / 1000.0;
            let duration_ms = duration.as_secs_f64() * 1000.0;
            match &result {
                Ok(guard) => {
                    if elapsed_ms > duration_ms / 2.0 {
                        log::warn!(
                            "Timeout lock {next_id} acquired after {elapsed_ms:.0}ms by {caller}"
                        );
                    } else {
                        log::trace!(
                            "Timeout lock {next_id} acquired after {elapsed_ms:.2}ms by {caller}"
                        );
                    }
                }
                Err(_) => {
                    log::error!(
                        "Timeout lock {next_id} timed out after {elapsed_ms:.0}ms for {caller}"
                    );
                }
            };
            futures::future::ready(result)
        })
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
