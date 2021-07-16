use anyhow::{anyhow, Result};
use derive_more::Display;
use futures::channel::oneshot::channel;
use shared_child::SharedChild;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[cfg(feature = "lock")]
pub mod lock;

#[cfg(unix)]
use {
    futures::future::{AbortHandle, Abortable},
    shared_child::unix::SharedChildExt,
};

pub trait ProcessGroupExt<T> {
    fn new_process_group(&mut self) -> &mut T;
}

impl ProcessGroupExt<Command> for Command {
    #[cfg(unix)]
    fn new_process_group(&mut self) -> &mut Command {
        // FIXME: Linux: refactor and use the tokio-process-ns crate

        use nix::Error;
        use std::io;
        use std::os::unix::process::CommandExt;

        unsafe {
            self.pre_exec(|| {
                nix::unistd::setsid().map_err(|e| match e {
                    Error::Sys(errno) => io::Error::from(errno),
                    error => io::Error::new(io::ErrorKind::Other, error),
                })?;
                Ok(())
            });
        }
        self
    }

    #[cfg(not(unix))]
    fn new_process_group(&mut self) -> &mut Command {
        self
    }
}

impl ProcessGroupExt<tokio::process::Command> for tokio::process::Command {
    #[cfg(unix)]
    fn new_process_group(&mut self) -> &mut tokio::process::Command {
        use nix::Error;
        use std::io;

        unsafe {
            self.pre_exec(|| {
                nix::unistd::setsid().map_err(|e| match e {
                    Error::Sys(errno) => io::Error::from(errno),
                    error => io::Error::new(io::ErrorKind::Other, error),
                })?;
                Ok(())
            });
        }
        self
    }

    #[cfg(not(unix))]
    fn new_process_group(&mut self) -> &mut tokio::process::Command {
        self
    }
}

#[derive(Display)]
pub enum ExeUnitExitStatus {
    #[display(fmt = "Aborted - {}", _0)]
    Aborted(std::process::ExitStatus),
    // workaround for goth being bound to previous stdlib Display impl for ExitCode
    // it was changed recently: https://github.com/rust-lang/rust/commit/11e40ce240d884303bee142a727decaeeef43bdb#diff-7015a38ee6056bbfa832b33281ffeaad5531c4dbfaff60ddfce0934475e040f4R532
    #[display(
        fmt = "Finished - exit code: {}",
        "_0.code().map(|code| format!(\"{}\", code)).unwrap_or(\"None\".into())"
    )]
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

    #[cfg(unix)]
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

        tokio::task::spawn_local(async move {
            tokio::time::sleep(timeout).await;
            abort_handle.abort();
        });

        let _ = Abortable::new(process.wait_until_finished(), abort_registration).await;
        self.check_if_running()
    }

    #[cfg(not(unix))]
    pub async fn terminate(&self, _timeout: Duration) -> Result<()> {
        // TODO: Implement termination for Windows
        Err(anyhow!(
            "Process termination not supported on non-UNIX systems"
        ))
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
