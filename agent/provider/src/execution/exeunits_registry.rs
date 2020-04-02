use anyhow::{anyhow, Result};
use log::info;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

use ya_core_model::activity;

use super::exeunit_instance::ExeUnitInstance;
//use ya_model::market::Agreement;

/// Descriptor of ExeUnit
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExeUnitDesc {
    name: String,
    path: PathBuf,
    args: Vec<String>,
    // Here other capabilities and exe units metadata.
}

/// Responsible for creating ExeUnits.
/// Stores registry of ExeUnits that can be created.
pub struct ExeUnitsRegistry {
    descriptors: HashMap<String, ExeUnitDesc>,
}

impl ExeUnitsRegistry {
    pub fn new() -> ExeUnitsRegistry {
        ExeUnitsRegistry {
            descriptors: HashMap::new(),
        }
    }

    pub fn spawn_exeunit(
        &self,
        name: &str,
        activity_id: &str,
        _agreement_id: &str,
    ) -> Result<ExeUnitInstance> {
        let working_dir = std::env::current_dir()?;
        let exeunit_desc = self.find_exeunit(name)?;
        let mut args = exeunit_desc.args.clone();
        // TODO: pass also agreement or its part with task_package
        args.push(activity_id.into());
        args.push(activity::local::BUS_ID.into());
        ExeUnitInstance::new(name, &exeunit_desc.path, &working_dir, &args)
    }

    pub fn register_exeunit(&mut self, desc: ExeUnitDesc) {
        info!(
            "Added [{}] ExeUnit to registry. Binary path: [{}].",
            desc.name,
            desc.path.display()
        );
        self.descriptors.insert(desc.name.clone(), desc);
    }

    pub fn register_exeunits_from_file(&mut self, path: &Path) -> Result<()> {
        let file = File::open(path).map_err(|error| {
            anyhow!(
                "Can't load ExeUnits to registry from file {}, error: {}.",
                path.display(),
                error
            )
        })?;

        let reader = BufReader::new(file);
        let descs: Vec<ExeUnitDesc> = serde_json::from_reader(reader).map_err(|error| {
            anyhow!(
                "Can't deserialize ExeUnits descriptors from file {}, error: {}.",
                path.display(),
                error
            )
        })?;

        for desc in descs.into_iter() {
            self.register_exeunit(desc);
        }
        Ok(())
    }

    pub fn find_exeunit(&self, name: &str) -> Result<ExeUnitDesc> {
        Ok(self
            .descriptors
            .get(name)
            .ok_or(anyhow!("ExeUnit [{}] doesn't exist in registry.", name))?
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resources_directory() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-resources/")
    }

    #[test]
    fn test_fill_registry_from_file() {
        let mut registry = ExeUnitsRegistry::new();
        registry
            .register_exeunits_from_file(&resources_directory().join("example-exeunits.json"))
            .unwrap();

        let dummy_desc = registry.find_exeunit("dummy").unwrap();
        assert_eq!(dummy_desc.name.as_str(), "dummy");
        assert_eq!(dummy_desc.path.to_str().unwrap(), "dummy.exe");

        let dummy_desc = registry.find_exeunit("wasm").unwrap();
        assert_eq!(dummy_desc.name.as_str(), "wasm");
        assert_eq!(dummy_desc.path.to_str().unwrap(), "wasm.exe");
    }

    #[test]
    fn test_fill_registry_from_local_exe_unit_descriotor() {
        let exe_units_descriptor = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../exe-unit/resources/local-exeunits-descriptor.json");
        let mut registry = ExeUnitsRegistry::new();
        registry
            .register_exeunits_from_file(&exe_units_descriptor)
            .unwrap();

        let dummy_desc = registry.find_exeunit("wasmtime").unwrap();
        assert_eq!(dummy_desc.name.as_str(), "wasmtime");
        assert_eq!(
            dummy_desc.path.to_str().unwrap(),
            "../target/debug/exe-unit"
        );
    }

    #[test]
    fn test_fill_registry_from_deb_exe_unit_descriotor() {
        let exe_units_descriptor = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../exe-unit/resources/exeunits-descriptor.json");
        let mut registry = ExeUnitsRegistry::new();
        registry
            .register_exeunits_from_file(&exe_units_descriptor)
            .unwrap();

        let dummy_desc = registry.find_exeunit("wasmtime").unwrap();
        assert_eq!(dummy_desc.name.as_str(), "wasmtime");
        assert_eq!(
            dummy_desc.path.to_str().unwrap(),
            "/usr/lib/yagna/plugins/exe-unit"
        );
    }
}
