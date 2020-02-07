use ya_exe_framework::ExeUnitFramework;
use ya_exe_framework::ExeUnit;

use anyhow::Result;

mod wasmtime_unit;
use wasmtime_unit::Wasmtime;
use crate::wasmtime_unit::WasmtimeFactory;


fn main() -> Result<()>  {
    env_logger::init();

    let framework = ExeUnitFramework::from_cmd_args(WasmtimeFactory::new())?;
    Ok(framework.run()?)
}
