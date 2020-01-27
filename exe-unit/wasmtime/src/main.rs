use ya_exe_framework::ExeUnitFramework;
use ya_exe_framework::ExeUnit;



pub struct Wasmtime;

impl Wasmtime {
    pub fn new() -> Box<dyn ExeUnit> {
        Box::new(Wasmtime)
    }
}

impl ExeUnit for Wasmtime {
}



fn main() {
    ExeUnitFramework::from_cmd_args(Wasmtime::new()).unwrap();
}
