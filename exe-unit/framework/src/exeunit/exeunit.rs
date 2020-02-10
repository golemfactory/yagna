
use anyhow::{Result};



/// Create ExeUnit and allowas to query useful information about it.
pub trait ExeUnitBuilder: std::marker::Send {
    fn create(&self) -> Result<Box<dyn ExeUnit>>;
}


/// Implement ExeUnit behavior..
pub trait ExeUnit {

    fn on_start(&mut self) -> Result<()>;
    fn on_deploy(&mut self, args: Vec<String>) -> Result<()>;
    fn on_run(&mut self, args: Vec<String>) -> Result<()>;
    fn on_transferred(&mut self) -> Result<()>;
    fn on_stop(&mut self) -> Result<()>;
}





