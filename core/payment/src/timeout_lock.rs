use futures::Future;
use std::ops::Deref;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::error::Elapsed;
use tokio::{
    sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::Timeout,
};
use uuid::Uuid;

use ya_persistence::executor::DbExecutor;

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

pub struct TimedMutex {
    mutex: Mutex<DbExecutor>,
    sender: Option<UnboundedSender<TimedMutexTaskMessage>>,
}

enum TimedMutexTaskMessage {
    Start(String),
    Finish,
}

pub struct TimedMutexGuard<'a> {
    mutex_guard: MutexGuard<'a, DbExecutor>,
    sender: &'a Option<UnboundedSender<TimedMutexTaskMessage>>,
}

impl Drop for TimedMutexGuard<'_> {
    fn drop(&mut self) {
        if let Some(sender) = &self.sender {
            if let Err(e) = sender.send(TimedMutexTaskMessage::Finish) {
                log::warn!("Cannot send finish to counter task {e}");
            }
        }
    }
}

impl<'a> Deref for TimedMutexGuard<'a> {
    type Target = MutexGuard<'a, DbExecutor>;

    fn deref(&self) -> &Self::Target {
        &self.mutex_guard
    }
}

impl TimedMutex {
    pub fn new(db: DbExecutor) -> Self {
        let (sender, mut receiver) =
            tokio::sync::mpsc::unbounded_channel::<TimedMutexTaskMessage>();

        tokio::spawn(async move {
            log::debug!("[TimedMutex] Counter thread started");
            loop {
                // wait for start or close without timeout
                let task_name = match receiver.recv().await {
                    None => break,
                    Some(TimedMutexTaskMessage::Start(x)) => x,
                    Some(TimedMutexTaskMessage::Finish) => {
                        log::warn!("[TimedMutex] Unexpected finish");
                        return;
                    }
                };

                log::info!("[TimedMutex] task {task_name} started...");
                let mut counter = 0;
                loop {
                    match tokio::time::timeout(Duration::from_secs(10), receiver.recv()).await {
                        Err(_) => {
                            log::warn!("[TimedMutex] Long running task: {task_name}!");
                        }
                        Ok(None) => log::warn!("[TimedMutex] Unexpected mpsc close."),
                        Ok(Some(TimedMutexTaskMessage::Finish)) => break,
                        Ok(Some(TimedMutexTaskMessage::Start(_))) => {
                            log::warn!("[TimedMutex] Unexpected start")
                        }
                    }
                }

                log::debug!("[TimedMutex] Timed task {task_name} finished.");
            }
            log::debug!("[TimedMutex] Counter thread finished");
        });

        Self {
            mutex: Mutex::new(db),
            sender: Some(sender),
        }
    }

    pub async fn timeout_lock(
        &self,
        duration: Duration,
        name: &str,
    ) -> Result<TimedMutexGuard<'_>, Elapsed> {
        let result = tokio::time::timeout(duration, self.mutex.lock())
            .await
            .map_err(|e| {
                log::warn!("Failed to lock mutex in scenario {0}", name);
                e
            })?;

        let id = Uuid::new_v4().to_simple().to_string();
        let task_id = format!("{name}::{id}");

        if let Some(sender) = &self.sender {
            if let Err(e) = sender.send(TimedMutexTaskMessage::Start(task_id)) {
                log::warn!("Cannot send start to counter task {name}: {e}");
            }
        }

        Ok(TimedMutexGuard {
            mutex_guard: result,
            sender: &self.sender,
        })
    }
}

impl Drop for TimedMutex {
    fn drop(&mut self) {
        self.sender.take();
    }
}
