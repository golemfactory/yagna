use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Child};

use anyhow::{Error, Result};
use serde::{Serialize, Deserialize};
use std::fs::File;
use std::io::BufReader;


/// Descriptor of ExeUnit
#[derive(Serialize, Deserialize)]
pub struct ExeUnitDesc {
    name: String,
    path: PathBuf,
    // Here other capabilities and exe units metadata.
}

/// Responsible for creating ExeUnits.
/// Stores registry of ExeUnits that can be created.
pub struct ExeUnitsRegistry {
    descriptors: HashMap<String, ExeUnitDesc>
}

/// TODO: Working ExeUnit instance
/// TODO: Move to separate file, when this class will be more functional.
pub struct ExeUnitInstance {
    process: Child
}


impl ExeUnitsRegistry {

    pub fn new() -> ExeUnitsRegistry {
        ExeUnitsRegistry{descriptors: HashMap::new()}
    }

    pub fn spawn_exeunit(&self, name: &str) -> Result<ExeUnitInstance> {
        let exeunit_desc = self.descriptors
            .get(name)
            .ok_or(Error::msg(format!("ExeUnit [{}] doesn't exist in registry.", name )))?;

        let mut child = Command::new(&exeunit_desc.name)
            .spawn()
            .map_err(|error| { Error::msg(format!("Can't spawn ExeUnit [{}]. Error: {}", name, error)) } )?;

        Ok(ExeUnitInstance{ process: child })
    }

    pub fn register_exeunit(&mut self, desc: ExeUnitDesc) {
        self.descriptors.insert(desc.name.clone(), desc);
    }

    pub fn register_exeunits_from_file(&mut self, path: &Path) -> Result<()> {
        let file = File::open(path)
            .map_err(|error|{
                Error::msg( format!("Can't load ExeUnits to registry from file {}, error: {}.", path.display(), error))
            })?;

        let reader = BufReader::new(file);
        let descs: Vec<ExeUnitDesc> = serde_json::from_reader(reader)
            .map_err(|error|{
                Error::msg( format!("Can't deserialize ExeUnits descriptors from file {}, error: {}.", path.display(), error))
            })?;

        for desc in descs.into_iter() {
            self.register_exeunit(desc);
        }
        Ok(())
    }

}

//#[cfg(test)]
//mod tests {
//    use super::*;
//
//    #[test]
//    fn test_fill_registry_from_file() {
//        let dict = {
//
//        };
//    }
//
//}
