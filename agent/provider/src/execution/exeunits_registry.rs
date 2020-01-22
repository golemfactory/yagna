use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;

/// Descriptor of ExeUnit
#[derive(Serialize, Deserialize, Clone)]
pub struct ExeUnitDesc {
    name: String,
    path: PathBuf,
    // Here other capabilities and exe units metadata.
}

/// Responsible for creating ExeUnits.
/// Stores registry of ExeUnits that can be created.
pub struct ExeUnitsRegistry {
    descriptors: HashMap<String, ExeUnitDesc>,
}

/// TODO: Working ExeUnit instance
/// TODO: Move to separate file, when this class will be more functional.
#[allow(dead_code)]
pub struct ExeUnitInstance {
    process: Child,
}

#[allow(dead_code)]
impl ExeUnitsRegistry {
    pub fn new() -> ExeUnitsRegistry {
        ExeUnitsRegistry {
            descriptors: HashMap::new(),
        }
    }

    pub fn spawn_exeunit(&self, name: &str) -> Result<ExeUnitInstance> {
        let exeunit_desc = self.find_exeunit(name)?;

        let child = Command::new(&exeunit_desc.name).spawn().map_err(|error| {
            Error::msg(format!("Can't spawn ExeUnit [{}]. Error: {}", name, error))
        })?;

        Ok(ExeUnitInstance { process: child })
    }

    pub fn register_exeunit(&mut self, desc: ExeUnitDesc) {
        self.descriptors.insert(desc.name.clone(), desc);
    }

    pub fn register_exeunits_from_file(&mut self, path: &Path) -> Result<()> {
        let file = File::open(path).map_err(|error| {
            Error::msg(format!(
                "Can't load ExeUnits to registry from file {}, error: {}.",
                path.display(),
                error
            ))
        })?;

        let reader = BufReader::new(file);
        let descs: Vec<ExeUnitDesc> = serde_json::from_reader(reader).map_err(|error| {
            Error::msg(format!(
                "Can't deserialize ExeUnits descriptors from file {}, error: {}.",
                path.display(),
                error
            ))
        })?;

        for desc in descs.into_iter() {
            self.register_exeunit(desc);
        }
        Ok(())
    }

    pub fn find_exeunit(&self, name: &str) -> Result<ExeUnitDesc> {
        Ok(self.descriptors
            .get(name)
            .ok_or(Error::msg(format!("ExeUnit [{}] doesn't exist in registry.", name)))?
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_resources_directory() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-resources/")
    }

    #[test]
    fn test_fill_registry_from_file() {
        let mut registry = ExeUnitsRegistry::new();
        registry.register_exeunits_from_file(&test_resources_directory().join("example-exeunits.json")).unwrap();

        let dummy_desc = registry.find_exeunit("dummy").unwrap();
        assert_eq!(dummy_desc.name.as_str(), "dummy");
        assert_eq!(dummy_desc.path.to_str().unwrap(), "dummy.exe");

        let dummy_desc = registry.find_exeunit("wasm").unwrap();
        assert_eq!(dummy_desc.name.as_str(), "wasm");
        assert_eq!(dummy_desc.path.to_str().unwrap(), "wasm.exe");
    }

}
