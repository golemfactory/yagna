use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use ya_client_model::NodeId;
use ya_core_model::payment::local as pay_local;
use ya_service_bus::{typed as bus, RpcEndpoint};

pub struct BatchCycleTaskManager {
    platforms: Vec<String>,
    owners: Vec<NodeId>,
    tasks: Vec<BatchCycleTask>,
}
impl BatchCycleTaskManager {
    pub fn new() -> Self {
        BatchCycleTaskManager {
            platforms: Vec::new(),
            owners: Vec::new(),
            tasks: Vec::new(),
        }
    }

    pub fn add_owner(&mut self, owner_id: NodeId) {
        log::info!("Adding owner: {}", owner_id);
        self.owners.push(owner_id);
        self.start_tasks_if_not_started();
    }
    pub fn add_platform(&mut self, platform: String) {
        log::info!("Adding platform: {}", platform);
        self.platforms.push(platform);
        self.start_tasks_if_not_started();
    }

    pub fn wake_owner_platform(&self, owner_id: NodeId, platform: String) {
        for task in self.tasks.iter() {
            if task.node_id == owner_id && task.platform == platform {
                task.waker.notify_waiters();
            }
        }
    }

    fn start_tasks_if_not_started(&mut self) {
        for owner_id in &self.owners {
            for platform in &self.platforms {
                if self
                    .tasks
                    .iter()
                    .any(|t| t.platform == *platform && t.node_id == *owner_id)
                {
                    continue;
                }
                self.tasks
                    .push(BatchCycleTask::new(*owner_id, platform.clone()));
            }
        }
    }

    async fn stop_tasks(&mut self) {
        for task in self.tasks.iter() {
            *task.finish.lock().unwrap() = true;
            task.waker.notify_waiters();
        }
        for task in self.tasks.drain(..) {
            task.handle.await.unwrap();
        }
    }
}

pub struct BatchCycleTask {
    node_id: NodeId,
    platform: String,
    waker: Arc<Notify>,
    finish: Arc<Mutex<bool>>,
    handle: tokio::task::JoinHandle<()>,
}

pub async fn sleep_after_error() {
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
}

impl BatchCycleTask {
    pub fn new(node_id: NodeId, platform: String) -> Self {
        let waker = Arc::new(Notify::new());
        let finish = Arc::new(Mutex::new(false));
        BatchCycleTask {
            node_id,
            platform: platform.clone(),
            waker: waker.clone(),
            finish: finish.clone(),
            handle: tokio::spawn(async move {
                log::info!(
                    "Starting batch cycle task for owner_id: {}, platform: {}",
                    node_id,
                    platform
                );
                let mut next_process: Option<DateTime<Utc>> = None;
                loop {
                    let now = Utc::now();
                    if let Some(next_process) = next_process {
                        if next_process > now {
                            let diff = (next_process - now).num_milliseconds();
                            if diff > 0 {
                                log::info!(
                                    "Sleeping for {} before next process for owner_id: {}, platform: {}",
                                    humantime::format_duration(std::time::Duration::from_secs_f64((diff as f64 / 1000.0).round())),
                                    node_id,
                                    platform
                                );
                                tokio::select! {
                                    _ = tokio::time::sleep(std::time::Duration::from_millis(diff as u64)) => {}
                                    _ = waker.notified() => {},
                                }
                            }
                        } else {
                            match bus::service(pay_local::BUS_ID)
                                .send(pay_local::ProcessPaymentsNow {
                                    node_id,
                                    platform: platform.clone(),
                                    skip_resolve: false,
                                    skip_send: false,
                                })
                                .await
                            {
                                Ok(Ok(_)) => {}
                                Ok(Err(e)) => {
                                    log::error!("Failed to process payments now: {:?}", e);
                                    // prevent busy loop
                                    sleep_after_error().await;
                                }
                                Err(e) => {
                                    log::error!("Failed to send process payments now: {:?}", e);
                                    // prevent busy loop
                                    sleep_after_error().await;
                                }
                            }
                        }
                    }
                    if *finish.lock().unwrap() {
                        break;
                    }

                    let info = match bus::service(pay_local::BUS_ID)
                        .send(pay_local::ProcessBatchCycleInfo {
                            node_id,
                            platform: platform.clone(),
                        })
                        .await
                    {
                        Ok(Ok(info)) => info,
                        Ok(Err(e)) => {
                            log::error!("Failed to get batch cycle info: {:?}", e);
                            // prevent busy loop
                            sleep_after_error().await;
                            continue;
                        }
                        Err(e) => {
                            log::error!("Failed to send batch cycle info: {:?}", e);
                            // prevent busy loop
                            sleep_after_error().await;
                            continue;
                        }
                    };

                    next_process = Some(info.next_process.and_utc());
                }
                log::info!(
                    "Batch cycle task finished for owner_id: {}, platform: {}",
                    node_id,
                    platform
                );
            }),
        }
    }
}
