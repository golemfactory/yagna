use anyhow::{anyhow, Result};
use derive_more::Display;
use std::path::{Path, PathBuf};
use tokio::process::{Child, Command};
use futures::future::{AbortHandle, Abortable};

/// Working ExeUnit instance representation.
#[derive(Display)]
#[display(fmt = "ExeUnit: name [{}]", name)]
pub struct ExeUnitInstance {
    name: String,
    #[allow(dead_code)]
    working_dir: PathBuf,

    abort_handle: AbortHandle,
    process: Option<Abortable<Child>>,
}

impl ExeUnitInstance {
    pub fn new(
        name: &str,
        binary_path: &Path,
        working_dir: &Path,
        args: &Vec<String>,
    ) -> Result<ExeUnitInstance> {
        log::info!("Spawning exeunit instance : {}", name);
        //        let child = Command::new(binary_path)
        let child = Command::new("echo")
            .args(args)
            .current_dir(working_dir)
            .kill_on_drop(true)
            .spawn() // FIXME -- this is not returning
            .map_err(|error| {
                anyhow!(
                    "Can't spawn ExeUnit [{}] from binary [{}] in working directory [{}]. Error: {}",
                    name, binary_path.display(), working_dir.display(), error
                )
            })?;
        log::info!("Exeunit process spawned, pid: {}", child.id());

        let (abort_handle, reg) = AbortHandle::new_pair();
        let process = Abortable::new(child, reg);

        let instance = ExeUnitInstance {
            name: name.to_string(),
            process: Some(process),
            abort_handle,
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

        // It requires kill_on_drop(true) to really kill process.
        // We don't call kill explicit, but process handle will be dropped
        // and so all references to this process.
        self.abort_handle.abort();
    }

    pub fn take_process_handle(&mut self) -> Result<Abortable<Child>> {
        self.process.take().ok_or(anyhow!("Process handle already taken."))
    }
}
