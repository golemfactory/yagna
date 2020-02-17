mod wasmtime_unit;
mod entrypoint;
mod manifest;

use anyhow::Result;
use structopt::StructOpt;

use crate::entrypoint::{ExeUnitMain, CmdArgs};


fn main() -> Result<()>  {
    env_logger::init();

    let cmdargs = CmdArgs::from_args();
    Ok(ExeUnitMain::entrypoint(cmdargs)?)
}
