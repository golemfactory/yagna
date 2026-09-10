use std::path::{Path, PathBuf};
use std::{fs, io};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use ya_utils_path::SwapSave;

use crate::events::Event;
use crate::startup_config::FileMonitor;

pub(crate) const SHUTDOWN_STATUS_JSON: &str = "shutdown-status.json";

/// Content of the shutdown-status file, through which a graceful shutdown
/// of a running provider can be requested (e.g. by `ya-provider shutdown`).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShutdownStatus {
    pub graceful_shutdown_requested: bool,
}

impl ShutdownStatus {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Ok(serde_json::from_reader(io::BufReader::new(
            fs::OpenOptions::new().read(true).open(path)?,
        ))?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(path.swap_save(serde_json::to_string_pretty(self)?)?)
    }
}

/// Watches the shutdown status file.
pub struct ShutdownManager {
    #[allow(dead_code)]
    monitor: Option<FileMonitor>,
    sender: Option<watch::Sender<Event>>,
    receiver: watch::Receiver<Event>,
}

impl ShutdownManager {
    /// Resets the status file immediately after the Provider acquires its
    /// process lock, so requests made during later startup are not lost.
    pub fn reset(shutdown_file: &Path) -> anyhow::Result<()> {
        ShutdownStatus::default().save(shutdown_file)
    }

    pub fn try_new(_shutdown_file: &Path) -> anyhow::Result<Self> {
        let (sender, receiver) = watch::channel(Event::Initialized);
        Ok(Self {
            monitor: None,
            sender: Some(sender),
            receiver,
        })
    }

    pub fn spawn_monitor(&mut self, shutdown_file: &Path) -> anyhow::Result<()> {
        let tx = self.sender.take().unwrap();
        let initial_tx = tx.clone();
        let handler = move |p: PathBuf| match ShutdownStatus::load(&p) {
            Ok(status) => {
                tx.send(Event::ShutdownChanged { status })
                    .unwrap_or_default();
            }
            Err(e) => log::warn!("Error reading shutdown status from {:?}: {:?}", p, e),
        };
        let monitor = FileMonitor::spawn(shutdown_file, FileMonitor::on_modified(handler))?;
        // A request can arrive after the process lock/reset but before the file
        // monitor starts. Read the current value after installing the watcher,
        // closing that startup window without losing later modifications.
        if let Ok(status) = ShutdownStatus::load(shutdown_file) {
            initial_tx
                .send(Event::ShutdownChanged { status })
                .unwrap_or_default();
        }
        self.monitor = Some(monitor);
        Ok(())
    }

    #[inline]
    pub fn event_receiver(&self) -> watch::Receiver<Event> {
        self.receiver.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_clears_stale_request() {
        let dir = tempdir::TempDir::new("shutdown-status").unwrap();
        let path = dir.path().join(SHUTDOWN_STATUS_JSON);

        ShutdownStatus {
            graceful_shutdown_requested: true,
        }
        .save(&path)
        .unwrap();

        ShutdownManager::reset(&path).unwrap();
        assert!(
            !ShutdownStatus::load(&path)
                .unwrap()
                .graceful_shutdown_requested
        );
    }

    #[test]
    fn monitor_observes_request_made_during_startup() {
        let dir = tempdir::TempDir::new("shutdown-status").unwrap();
        let path = dir.path().join(SHUTDOWN_STATUS_JSON);
        ShutdownStatus {
            graceful_shutdown_requested: true,
        }
        .save(&path)
        .unwrap();

        let mut manager = ShutdownManager::try_new(&path).unwrap();
        manager.spawn_monitor(&path).unwrap();

        assert!(matches!(
            &*manager.event_receiver().borrow(),
            Event::ShutdownChanged { status } if status.graceful_shutdown_requested
        ));
    }
}
