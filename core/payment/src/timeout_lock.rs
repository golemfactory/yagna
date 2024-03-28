use std::time::Duration;

use futures::Future;
use tokio::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub trait MutexTimeoutExt<T: ?Sized + 'static> {
    fn timeout_lock_with_log(
        &self,
        params: TimeoutLogParams,
    ) -> impl Future<Output = Result<MutexGuard<'_, T>, tokio::time::error::Elapsed>> + Send;
}

impl<T: ?Sized + 'static + Send + Sync> MutexTimeoutExt<T> for Mutex<T> {
    async fn timeout_lock_with_log(
        &self,
        params: TimeoutLogParams,
    ) -> Result<MutexGuard<'_, T>, tokio::time::error::Elapsed> {
        let time_start = std::time::Instant::now();
        let res = tokio::time::timeout(params.error_timeout, self.lock()).await;
        let secs_elapsed = time_start.elapsed().as_secs_f64();
        write_sync_log(res.is_err(), secs_elapsed, params);
        res
    }
}

pub struct TimeoutLogParams {
    /// Log topic
    pub topic: &'static str,
    /// Log level for warning message
    pub log_level_warning: log::Level,
    /// Log level for timeout message
    pub log_level_error: log::Level,
    /// Timeout for warning message (operation still completed successfully)
    pub warning_timeout: Duration,
    /// Timeout for error message and timeout
    pub error_timeout: Duration,
}

impl Default for TimeoutLogParams {
    fn default() -> Self {
        Self {
            topic: "Generic lock timeout",
            log_level_warning: log::Level::Warn,
            log_level_error: log::Level::Error,
            warning_timeout: Duration::from_secs(5),
            error_timeout: Duration::from_secs(30),
        }
    }
}

pub trait RwLockTimeoutExt<T: ?Sized + 'static> {
    fn timeout_read_with_log(
        &self,
        params: TimeoutLogParams,
    ) -> impl Future<Output = Result<RwLockReadGuard<'_, T>, tokio::time::error::Elapsed>> + Send;

    fn timeout_write_with_log(
        &self,
        params: TimeoutLogParams,
    ) -> impl Future<Output = Result<RwLockWriteGuard<'_, T>, tokio::time::error::Elapsed>> + Send;
}

impl<T: ?Sized + 'static + Send + Sync> RwLockTimeoutExt<T> for RwLock<T> {
    async fn timeout_read_with_log(
        &self,
        params: TimeoutLogParams,
    ) -> Result<RwLockReadGuard<'_, T>, tokio::time::error::Elapsed> {
        let time_start = std::time::Instant::now();
        let res = tokio::time::timeout(params.error_timeout, self.read()).await;
        let secs_elapsed = time_start.elapsed().as_secs_f64();
        write_sync_log(res.is_err(), secs_elapsed, params);
        res
    }

    async fn timeout_write_with_log(
        &self,
        params: TimeoutLogParams,
    ) -> Result<RwLockWriteGuard<'_, T>, tokio::time::error::Elapsed> {
        let time_start = std::time::Instant::now();
        let res = tokio::time::timeout(params.error_timeout, self.write()).await;
        let secs_elapsed = time_start.elapsed().as_secs_f64();
        write_sync_log(res.is_err(), secs_elapsed, params);
        res
    }
}

fn write_sync_log(is_err: bool, secs_elapsed: f64, params: TimeoutLogParams) {
    if is_err {
        if params.error_timeout.as_secs_f64() > 1.0 {
            log::log!(
                params.log_level_error,
                "Timeout {:.1}s - {}",
                secs_elapsed,
                params.topic
            );
        } else {
            log::log!(
                params.log_level_error,
                "Timeout {:.1}ms - {}",
                secs_elapsed / 1000.0,
                params.topic
            );
        }
    }
    if secs_elapsed > params.warning_timeout.as_secs_f64() {
        if params.warning_timeout.as_secs_f64() > 1.0 {
            log::log!(
                params.log_level_warning,
                "Long timeout warning {:.1}s - {}",
                secs_elapsed,
                params.topic
            );
        } else {
            log::log!(
                params.log_level_warning,
                "Long timeout warning {:.1}ms - {}",
                secs_elapsed / 1000.0,
                params.topic
            );
        }
    }
}
