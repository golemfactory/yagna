use ya_exe_framework::ExeUnitFramework;
use ya_exe_framework::ExeUnit;

use anyhow::Result;

pub struct Wasmtime;

impl Wasmtime {
    pub fn new() -> Box<dyn ExeUnit> {
        Box::new(Wasmtime)
    }
}

impl ExeUnit for Wasmtime {
}



fn main() -> Result<()>  {
    env_logger::init();

    let framework = ExeUnitFramework::from_cmd_args(Wasmtime::new())?;
    Ok(framework.run()?)
}
