use anyhow::{anyhow, Result};
use log_derive::{logfn, logfn_inputs};
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

// TODO: should check spawned process state and report it back via GSB
impl ExeUnitInstance {
    #[logfn_inputs(Debug, fmt = "Spawning ExeUnit: {}, bin={:?}, wd={:?}, args={:?}")]
    #[logfn(Debug, fmt = "ExeUnit spawned: {:?}")]
    pub fn new(
        name: &str,
        binary_path: &Path,
        working_dir: &Path,
        args: &Vec<String>,
    ) -> Result<ExeUnitInstance> {
        let child = Command::new(binary_path)
//        let child = Command::new("echo")
            .args(args)
            .current_dir(working_dir)
            .spawn()
            .map_err(|error| {
                anyhow!(
                    "Can't spawn ExeUnit [{}] from binary [{}] in working directory [{}]. Error: {}",
                    name, binary_path.display(), working_dir.display(), error
                )
            })?;
        log::debug!("ExeUnit spawned, pid: {}", child.id());

        let instance = ExeUnitInstance {
            name: name.to_string(),
            process: child,
            working_dir: working_dir.to_path_buf(),
        };

        Ok(instance)
    }

    #[logfn_inputs(Debug, fmt = "Killing ExeUnit: {:?}")]
    #[logfn(Debug, fmt = "exeunit killed: {:?}")]
    pub fn kill(&mut self) {
        if let Err(_error) = self.process.kill() {
            log::warn!("Process wasn't running.");
        }
    }
}
