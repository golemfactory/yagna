use anyhow::{anyhow, Result};
use derive_more::Display;
use futures::channel::oneshot::channel;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use shared_child::SharedChild;
use std::sync::Arc;
use std::thread;


/// Working ExeUnit instance representation.
#[derive(Display)]
#[display(fmt = "ExeUnit: name [{}]", name)]
pub struct ExeUnitInstance {
    name: String,
    #[allow(dead_code)]
    working_dir: PathBuf,
    process: Arc<SharedChild>,
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

pub struct ProcessHandle {
    process: Arc<SharedChild>,
}

impl ExeUnitInstance {
    pub fn new(
        name: &str,
        binary_path: &Path,
        working_dir: &Path,
        _args: &Vec<String>,
    ) -> Result<ExeUnitInstance> {
        log::info!("Spawning exeunit instance : {}", name);
        //        let child = Command::new(binary_path)
        let mut command = Command::new("sleep");
        command
            .args(vec!["5000"])
            //.args(args)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let child = Arc::new(SharedChild::spawn(&mut command)
            .map_err(|error| {
                anyhow!(
                        "Can't spawn ExeUnit [{}] from binary [{}] in working directory [{}]. Error: {}",
                        name, binary_path.display(), working_dir.display(), error
                    )
            })?);

        log::info!("Exeunit process spawned, pid: {}", child.id());

        let instance = ExeUnitInstance {
            name: name.to_string(),
            process: child,
            working_dir: working_dir.to_path_buf(),
        };
        log::info!(
            "Exeunit instance [{}] spawned in workdir {}",
            &instance.name,
            &instance.working_dir.display()
        );

        Ok(instance)
    }

    pub fn kill(&self) {
        log::info!("Killing ExeUnit [{}]...", &self.name);
        let _ = self.process.kill();
    }

    pub fn get_process_handle(&self) -> ProcessHandle {
        ProcessHandle{process: self.process.clone()}
    }
}

impl ProcessHandle {
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

        return receiver.await.unwrap();
    }
}

