use actix::prelude::*;
use anyhow::{anyhow, Result};
use derive_more::Display;
use futures::channel::oneshot::channel;
use futures::future::{AbortHandle, Abortable};
use shared_child::unix::SharedChildExt;
use shared_child::SharedChild;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Display)]
pub enum ExeUnitExitStatus {
    #[display(fmt = "Aborted - {}", _0)]
    Aborted(std::process::ExitStatus),
    #[display(fmt = "Finished - {}", _0)]
    Finished(std::process::ExitStatus),
    #[display(fmt = "Error - {}", _0)]
    Error(std::io::Error),
}

#[derive(Clone)]
pub struct ProcessHandle {
    process: Arc<SharedChild>,
}

impl ProcessHandle {
    pub fn new(mut command: &mut Command) -> Result<ProcessHandle> {
        Ok(ProcessHandle {
            process: Arc::new(SharedChild::spawn(&mut command)?),
        })
    }

    pub fn kill(&self) {
        let _ = self.process.kill();
    }

    pub fn pid(&self) -> u32 {
        self.process.id()
    }

    /// TODO: Unix specific code. Support windows in future.
    pub async fn terminate(&self, timeout: Duration) -> Result<()> {
        let process = self.process.clone();
        if let Err(_) = process.send_signal(libc::SIGTERM) {
            // Error means, that probably process was already terminated, because:
            // - We have permissions to send signal, since we created this process.
            // - We specified correct signal SIGTERM.
            // But better let's check.
            return self.check_if_running();
        }

        let process = self.clone();
        let (abort_handle, abort_registration) = AbortHandle::new_pair();

        Arbiter::spawn(async move {
            tokio::time::delay_for(timeout).await;
            abort_handle.abort();
        });

        let _ = Abortable::new(process.wait_until_finished(), abort_registration).await;
        self.check_if_running()
    }

    pub fn check_if_running(&self) -> Result<()> {
        let terminate_result = self.process.try_wait();
        match terminate_result {
            Ok(expected_status) => match expected_status {
                // Process already exited. Terminate was successful.
                Some(_status) => Ok(()),
                None => Err(anyhow!(
                    "Process [pid={}] is still running.",
                    self.process.id()
                )),
            },
            Err(error) => Err(anyhow!(
                "Failed to wait for process [pid={}]. Error: {}",
                self.process.id(),
                error
            )),
        }
    }

    pub async fn wait_until_finished(self) -> ExeUnitExitStatus {
        let process = self.process.clone();
        let (sender, receiver) = channel::<ExeUnitExitStatus>();

        thread::spawn(move || {
            let result = process.wait();

            let status = match result {
                Ok(status) => match status.code() {
                    // status.code() will return None in case of termination by signal.
                    None => ExeUnitExitStatus::Aborted(status),
                    Some(_code) => ExeUnitExitStatus::Finished(status),
                },
                Err(error) => ExeUnitExitStatus::Error(error),
            };
            sender.send(status)
        });

        // Note: unwrap can't fail here. All sender, receiver and thread will
        // end their lifetime before await will return. There's no danger
        // that one of them will be dropped earlier.
        return receiver.await.unwrap();
    }
}
