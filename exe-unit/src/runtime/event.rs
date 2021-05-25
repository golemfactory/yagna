use futures::channel::mpsc::{channel, Receiver, Sender};
use futures::future::BoxFuture;
use futures::{FutureExt, SinkExt};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use ya_runtime_api::server::{ProcessStatus, RuntimeEvent};

#[derive(Clone)]
pub struct EventMonitor {
    inner: Arc<Mutex<HashMap<u64, EventChannel>>>,
}

pub struct EventReceiver {
    pub rx: Receiver<ProcessStatus>,
    pid: u64,
    monitor: EventMonitor,
}

impl Drop for EventReceiver {
    fn drop(&mut self) {
        self.monitor.remove(self.pid);
    }
}

#[allow(unused)]
struct EventChannel {
    tx: Sender<ProcessStatus>,
    rx: Option<Receiver<ProcessStatus>>,
}

impl Default for EventChannel {
    fn default() -> Self {
        let (tx, rx) = channel(1);
        EventChannel { tx, rx: Some(rx) }
    }
}

impl EventMonitor {
    pub fn events(&mut self, pid: u64) -> Option<EventReceiver> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .entry(pid)
            .or_insert_with(Default::default)
            .rx
            .take()
            .map(|rx| EventReceiver {
                rx,
                pid,
                monitor: self.clone(),
            })
    }

    pub fn remove(&mut self, pid: u64) {
        self.inner.lock().unwrap().remove(&pid);
    }
}

impl Default for EventMonitor {
    fn default() -> Self {
        EventMonitor {
            inner: Arc::new(Mutex::new(Default::default())),
        }
    }
}

impl RuntimeEvent for EventMonitor {
    fn on_process_status<'a>(&self, status: ProcessStatus) -> BoxFuture<'a, ()> {
        let mut tx = {
            self.inner
                .lock()
                .unwrap()
                .entry(status.pid)
                .or_insert_with(Default::default)
                .tx
                .clone()
        };
        async move {
            if let Err(err) = tx.send(status).await {
                log::error!("Event channel error: {:?}", err);
            }
        }
        .boxed()
    }
}
