use anyhow::{anyhow, Result};
use log::info;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
//TODO: use tokio::process::{Child, Command};

/// Working ExeUnit instance representation.
#[derive(Debug)]
pub struct ExeUnitInstance {
    name: String,
    process: Child,
    #[allow(dead_code)]
    working_dir: PathBuf,
}

impl ExeUnitInstance {
    pub fn new(
        name: &str,
        binary_path: &Path,
        working_dir: &Path,
        args: &Vec<String>,
    ) -> Result<ExeUnitInstance> {
        info!("spawning exeunit instance : {}", name);
        //        let child = Command::new(binary_path)
        let child = Command::new("echo")
            .args(args)
            .current_dir(working_dir)
            .spawn() // FIXME -- this is not returning
            .map_err(|error| {
                anyhow!(
                    "Can't spawn ExeUnit [{}] from binary [{}] in working directory [{}]. Error: {}",
                    name, binary_path.display(), working_dir.display(), error
                )
            })?;
        info!("exeunit spawned, pid: {}", child.id());

        let instance = ExeUnitInstance {
            name: name.to_string(),
            process: child,
            working_dir: working_dir.to_path_buf(),
        };
        info!("exeunit instance spawned: {:?}", instance);

        Ok(instance)
    }

    pub fn kill(&mut self) {
        info!("Killing ExeUnit [{}]...", &self.name);
        if let Err(_error) = self.process.kill() {
            info!("Process wasn't running.");
        }
    }
}
