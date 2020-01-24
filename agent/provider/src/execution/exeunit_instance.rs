use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use anyhow::{Error, Result};
use log::info;

/// Working ExeUnit instance representation.
pub struct ExeUnitInstance {
    name: String,
    process: Child,
    #[allow(dead_code)]
    working_dir: PathBuf,
}

impl ExeUnitInstance {
    pub fn new(
        name: &str,
        path: &Path,
        working_dir: &Path,
        args: &Vec<String>,
    ) -> Result<ExeUnitInstance> {
        let child = Command::new(path)
            .args(args)
            .current_dir(working_dir)
            .spawn()
            .map_err(|error| {
                Error::msg(format!(
                    "Can't spawn ExeUnit [{}] in working directory [{}]. Error: {}",
                    name, working_dir, error
                ))
            })?;

        Ok(ExeUnitInstance {
            name: name.to_string(),
            process: child,
            working_dir: working_dir.to_path_buf(),
        })
    }

    pub fn kill(&mut self) {
        info!("Killing ExeUnit [{}]...", &self.name);
        if let Err(_error) = self.process.kill() {
            info!("Process wasn't running.");
        }
    }
}
