use std::collections::HashMap;
use std::path::Path;


/// Descriptor of ExeUnit
struct ExeUnitDesc {
    name: String,
    path: Path
}

/// Responsible for creating ExeUnits.
/// Stores registry of ExeUnits that can be created.
struct ExeUnitsRegistry {
    descriptors: HashMap<String, ExeUnitDesc>
}

/// TODO: Working ExeUnit instance
struct ExeUnitInstance;


impl ExeUnitsRegistry {

    pub fn new() -> ExeUnitsRegistry {
        ExeUnitsRegistry{descriptors: HashMap::new()}
    }

    pub fn spawn_exeunit() -> ExeUnitInstance {
        ExeUnitInstance{}
    }

    ///TODO:
    pub fn register_exeunit() {
        unimplemented!();
    }
}
