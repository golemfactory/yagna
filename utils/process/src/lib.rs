use anyhow::{anyhow, Result};
use derive_more::Display;
use futures::channel::oneshot::channel;
use futures::future::{AbortHandle, Abortable};
use shared_child::SharedChild;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[cfg(feature = "lock")]
pub mod lock;

#[cfg(unix)]
use shared_child::unix::SharedChildExt;

#[cfg(windows)]
use std::process::ExitStatus;
#[cfg(windows)]
use winapi::um::{
    errhandlingapi,
    wincon::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT},
};

pub trait ProcessGroupExt<T> {
    fn new_process_group(&mut self) -> &mut T;
}

impl ProcessGroupExt<Command> for Command {
    #[cfg(unix)]
    fn new_process_group(&mut self) -> &mut Command {
        // FIXME: Linux: refactor and use the tokio-process-ns crate

        use std::io;
        use std::os::unix::process::CommandExt;

        unsafe {
            self.pre_exec(|| {
                nix::unistd::setsid().map_err(io::Error::from)?;
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
        use std::io;

        unsafe {
            self.pre_exec(|| {
                nix::unistd::setsid().map_err(io::Error::from)?;
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
    pub fn new(command: &mut Command) -> Result<ProcessHandle> {
        Ok(ProcessHandle {
            process: Arc::new(SharedChild::spawn(command)?),
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
        if process.send_signal(libc::SIGTERM).is_err() {
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

    #[cfg(windows)]
    pub async fn terminate(&self, timeout: Duration) -> Result<()> {
        use anyhow::bail;

        let process = self.process.clone();
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let _process_pid = process.id();

        tokio::task::spawn_local(async move {
            tokio::time::sleep(timeout).await;
            abort_handle.abort();
        });

        if let Ok(Err(err)) = Abortable::new(
            async { unsafe { ctrl_break_process(process) } },
            abort_registration,
        )
        .await
        {
            bail!(err.context("Failed to CTRL-BREAK process"));
        }

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
        receiver.await.unwrap()
    }
}

#[cfg(windows)]
unsafe fn ctrl_break_process(process: Arc<SharedChild>) -> Result<ExitStatus> {
    let process_pid = process.id();
    let event_result = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, process_pid);

    if event_result == 0 {
        return Err(anyhow!(
            "Unable to send CTRL-BREAK event to the process. Error code: {}",
            errhandlingapi::GetLastError()
        ));
    };
    Ok(process.wait()?)
}
