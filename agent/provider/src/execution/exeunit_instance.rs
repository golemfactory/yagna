use anyhow::{anyhow, Result};
use derive_more::Display;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use ya_utils_process::{ProcessGroupExt, ProcessHandle};

/// Working ExeUnit instance representation.
#[derive(Display)]
#[display(fmt = "ExeUnit: name [{}]", name)]
pub struct ExeUnitInstance {
    name: String,
    #[allow(dead_code)]
    working_dir: PathBuf,
    process_handle: ProcessHandle,
}

impl ExeUnitInstance {
    pub fn new(
        name: &str,
        binary_path: &Path,
        working_dir: &Path,
        args: &Vec<String>,
    ) -> Result<ExeUnitInstance> {
        log::info!("Spawning exeunit instance: {}", name);
        log::debug!("Spawning args: {:?}", args);
        log::debug!("Spawning in: {:?}", working_dir);

        log::info!(
            "Exeunit log directory: {}",
            working_dir.join("logs").display()
        );

        let binary_path = ya_utils_path::normalize_path(&binary_path)
            .map_err(|e| anyhow!("Failed to spawn [{}]: {}", binary_path.display(), e))?;

        let mut command = Command::new(&binary_path);
        command
            .args(args)
            .current_dir(working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // new_process_group is a no-op on non-Unix systems
            .new_process_group();

        let child = ProcessHandle::new(&mut command).map_err(|error| {
            anyhow!(
                "Can't spawn ExeUnit [{}] from binary [{}] in working directory [{}]. Error: {}",
                name,
                binary_path.display(),
                working_dir.display(),
                error
            )
        })?;

        log::info!("Exeunit process spawned, pid: {}", child.pid());

        let instance = ExeUnitInstance {
            name: name.to_string(),
            process_handle: child,
            working_dir: working_dir.to_path_buf(),
        };

        Ok(instance)
    }

    pub async fn run_with_output(
        binary_path: &Path,
        working_dir: &Path,
        args: Vec<String>,
    ) -> Result<String> {
        log::info!("Running ExeUnit: {}", binary_path.display());
        log::debug!("Running args: {:?}", args);
        log::debug!("Running in: {:?}", working_dir);

        let binary_path = ya_utils_path::normalize_path(&binary_path)
            .map_err(|e| anyhow!("Failed to run [{}]: {}", binary_path.display(), e))?;
        let mut command = tokio::process::Command::new(&binary_path);

        let child = command
            .args(args)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            // new_process_group is a no-op on non-Unix systems
            .new_process_group()
            .spawn()
            .map_err(|error| {
                anyhow!(
                    "Can't run ExeUnit [{}] in working directory [{}]. Error: {}",
                    binary_path.display(),
                    working_dir.display(),
                    error
                )
            })?;

        let output = child.wait_with_output().await?;
        Ok(String::from_utf8_lossy(output.stdout.as_slice()).to_string())
    }

    pub fn kill(&self) {
        log::info!("Killing ExeUnit [{}]... pid: {}", &self.name, self.pid());
        self.process_handle.kill();
    }

    pub async fn terminate(&self, timeout: Duration) -> Result<()> {
        log::info!(
            "Terminating ExeUnit [{}]... pid: {}",
            &self.name,
            self.pid()
        );
        self.process_handle.terminate(timeout).await
    }

    pub fn get_process_handle(&self) -> ProcessHandle {
        self.process_handle.clone()
    }

    fn pid(&self) -> u32 {
        self.process_handle.pid()
    }
}
