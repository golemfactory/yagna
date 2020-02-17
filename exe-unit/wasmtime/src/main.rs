mod wasmtime_unit;
mod entrypoint;

use anyhow::{Result, Error};
use structopt::StructOpt;

use crate::wasmtime_unit::WasmtimeFactory;
use crate::entrypoint::{ExeUnitMain, CmdArgs};


fn main() -> Result<()>  {
    env_logger::init();

    let cmdargs = CmdArgs::from_args();
    Ok(ExeUnitMain::entrypoint(cmdargs)?)
}
