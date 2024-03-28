use std::time::Duration;

use futures::Future;
use tokio::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub trait MutexTimeoutExt<T: ?Sized + 'static> {
    fn timeout_lock_with_log(
        &self,
        params: &TimeoutLogParams,
    ) -> impl Future<Output = Result<MutexGuard<'_, T>, tokio::time::error::Elapsed>> + Send;
}

impl<T: ?Sized + 'static + Send + Sync> MutexTimeoutExt<T> for Mutex<T> {
    async fn timeout_lock_with_log(
        &self,
        params: &TimeoutLogParams,
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
        params: &TimeoutLogParams,
    ) -> impl Future<Output = Result<RwLockReadGuard<'_, T>, tokio::time::error::Elapsed>> + Send;

    fn timeout_write_with_log(
        &self,
        params: &TimeoutLogParams,
    ) -> impl Future<Output = Result<RwLockWriteGuard<'_, T>, tokio::time::error::Elapsed>> + Send;
}

impl<T: ?Sized + 'static + Send + Sync> RwLockTimeoutExt<T> for RwLock<T> {
    async fn timeout_read_with_log(
        &self,
        params: &TimeoutLogParams,
    ) -> Result<RwLockReadGuard<'_, T>, tokio::time::error::Elapsed> {
        let time_start = std::time::Instant::now();
        let res = tokio::time::timeout(params.error_timeout, self.read()).await;
        let secs_elapsed = time_start.elapsed().as_secs_f64();
        write_sync_log(res.is_err(), secs_elapsed, params);
        res
    }

    async fn timeout_write_with_log(
        &self,
        params: &TimeoutLogParams,
    ) -> Result<RwLockWriteGuard<'_, T>, tokio::time::error::Elapsed> {
        let time_start = std::time::Instant::now();
        let res = tokio::time::timeout(params.error_timeout, self.write()).await;
        let secs_elapsed = time_start.elapsed().as_secs_f64();
        write_sync_log(res.is_err(), secs_elapsed, params);
        res
    }
}

fn write_sync_log(is_err: bool, secs_elapsed: f64, params: &TimeoutLogParams) {
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

mod test {
    use crate::timeout_lock::{MutexTimeoutExt, RwLockTimeoutExt, TimeoutLogParams};
    use log::{Level, Record};
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::Duration;
    use tokio::sync::{Mutex, RwLock};

    // Mock logger to capture log messages
    struct MockLogger(StdMutex<Vec<String>>);

    impl log::Log for MockLogger {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }

        fn log(&self, record: &Record) {
            let mut logs = self.0.lock().unwrap();
            logs.push(format!("{}", record.args()));
        }

        fn flush(&self) {}
    }

    static LOGGER: MockLogger = MockLogger(StdMutex::new(Vec::new()));

    // Initialize logger before tests run
    fn init_logger() {
        log::set_logger(&LOGGER).expect("Failed to set logger");
        log::set_max_level(Level::Trace.to_level_filter());
    }

    #[tokio::test]
    async fn test_mutex_timeout_lock_with_log() {
        init_logger();

        // Create a Mutex with a timeout
        let mutex = Mutex::new(());
        {
            // Lock the mutex - should be ok
            let result = mutex.timeout_lock_with_log(&TimeoutLogParams::default()).await;
            assert!(result.is_ok());

            assert!(LOGGER
                .0
                .lock()
                .unwrap()
                .iter()
                .all(|log| log.is_empty()));
        }

        let params = super::TimeoutLogParams {
            error_timeout: Duration::from_millis(10),
            warning_timeout: Duration::from_millis(0),
            ..Default::default()
        };

        // Lock the mutex with a short timeout (should log a warning)
        let result = mutex.timeout_lock_with_log(&params).await;
        assert!(result.is_ok());

        // Ensure a warning log is captured
        assert!(LOGGER
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|log| log.contains("Long timeout warning")));

        // Lock the mutex with an even shorter timeout (should log an error)
        let result = mutex.timeout_lock_with_log(&params).await;
        assert!(result.is_err());

        // Ensure an error log is captured
        assert!(LOGGER
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|log| log.contains("Timeout")));
    }

    #[tokio::test]
    async fn test_rwlock_timeout_read_with_log() {
        init_logger();

        // Create an RwLock with a timeout
        let rwlock = Arc::new(RwLock::new(()));

        // Perform a read operation with default parameters
        let result = rwlock
            .timeout_read_with_log(&TimeoutLogParams::default())
            .await;
        assert!(result.is_ok());

        // Ensure no logs are captured for successful operation
        assert!(LOGGER.0.lock().unwrap().is_empty());

        // Parameters for a short timeout
        let params = TimeoutLogParams {
            error_timeout: Duration::from_millis(10),
            warning_timeout: Duration::from_millis(0),
            ..Default::default()
        };

        // Perform a read operation with a short timeout (should log a warning)
        let result = rwlock.timeout_read_with_log(&params).await;
        assert!(result.is_ok());

        // Ensure a warning log is captured
        assert!(LOGGER
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|log| log.contains("Long timeout warning")));

        // Perform a read operation with an even shorter timeout (should log an error)
        let result = rwlock.timeout_read_with_log(&params).await;
        assert!(result.is_err());

        // Ensure an error log is captured
        assert!(LOGGER
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|log| log.contains("Timfeout")));
    }

    #[tokio::test]
    async fn test_rwlock_timeout_write_with_log() {
        init_logger();

        // Create an RwLock with a timeout
        let rwlock = Arc::new(RwLock::new(()));

        // Perform a write operation with default parameters
        let result = rwlock
            .timeout_write_with_log(&TimeoutLogParams::default())
            .await;
        assert!(result.is_ok());

        // Ensure no logs are captured for successful operation
        assert!(LOGGER.0.lock().unwrap().is_empty());

        // Parameters for a short timeout
        let params = TimeoutLogParams {
            error_timeout: Duration::from_millis(10),
            warning_timeout: Duration::from_millis(0),
            ..Default::default()
        };

        // Perform a write operation with a short timeout (should log a warning)
        let result = rwlock.timeout_write_with_log(&params).await;
        assert!(result.is_ok());

        // Ensure a warning log is captured
        assert!(LOGGER
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|log| log.contains("Long timeout warning")));

        // Perform a write operation with an even shorter timeout (should log an error)
        let result = rwlock.timeout_write_with_log(&params).await;
        assert!(result.is_err());

        // Ensure an error log is captured
        assert!(LOGGER
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|log| log.contains("Timeout")));
    }
}
