
use anyhow::{Result};
use std::path::PathBuf;


pub struct DirectoryMount {
    pub host: PathBuf,
    pub guest: PathBuf,
}


/// Create ExeUnit and allowas to query useful information about it.
pub trait ExeUnitBuilder: std::marker::Send {
    fn create(&self, mounts: Vec<DirectoryMount>) -> Result<Box<dyn ExeUnit>>;
}


/// Implement ExeUnit behavior..
pub trait ExeUnit {

    fn on_start(&mut self) -> Result<()>;
    fn on_deploy(&mut self, args: Vec<String>) -> Result<()>;
    fn on_run(&mut self, args: Vec<String>) -> Result<()>;
    fn on_transferred(&mut self) -> Result<()>;
    fn on_stop(&mut self) -> Result<()>;
}





