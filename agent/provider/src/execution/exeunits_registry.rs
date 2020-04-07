use anyhow::{anyhow, Result};
use log::info;
use path_clean::PathClean;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

use super::exeunit_instance::ExeUnitInstance;

/// Descriptor of ExeUnit
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct ExeUnitDesc {
    name: String,
    supervisor_path: PathBuf,
    runtime_path: PathBuf,

    // Here other capabilities and exe units metadata.
    #[serde(default = "default_description")]
    description: String,
}

fn default_description() -> String {
    "No description provided.".to_string()
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
        args: Vec<String>,
        working_dir: &Path,
    ) -> Result<ExeUnitInstance> {
        let exeunit_desc = self.find_exeunit(name)?;

        // Add to arguments path to runtime, which should be spawned
        // by supervisor as subprocess.

        // TODO: I'm not sure, if we should do it. Supervisor don't have to use
        //       runtime path at all. This path can be invalid and execution could
        //       be still correct. But as long as there's only one runtime, better
        //       handle this here.
        let runtime_path = exeunit_desc
            .runtime_path
            .to_str()
            .ok_or(anyhow!(
                "ExeUnit runtime path [{}] contains invalid characters.",
                &exeunit_desc.runtime_path.display()
            ))?
            .to_string();

        let mut extended_args = vec!["-b".to_owned(), runtime_path];

        // Add arguments from front. ExeUnit api requires positional
        // arguments, so ExeUnit can add only non-positional args.
        extended_args.extend(args);

        ExeUnitInstance::new(
            name,
            &exeunit_desc.supervisor_path,
            &working_dir,
            &extended_args,
        )
    }

    pub fn register_exeunit(&mut self, mut desc: ExeUnitDesc) -> Result<()> {
        desc.supervisor_path = normalize_path(&desc.supervisor_path)?;
        desc.runtime_path = normalize_path(&desc.runtime_path)?;

        info!(
            "Added [{}] ExeUnit to registry. Supervisor path: [{}], Runtime path: [{}].",
            desc.name,
            desc.supervisor_path.display(),
            desc.runtime_path.display()
        );
        self.descriptors.insert(desc.name.clone(), desc);
        Ok(())
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
            self.register_exeunit(desc)?
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

fn normalize_path(path: &Path) -> Result<PathBuf> {
    let current_dir = std::env::current_dir()?;

    let mut path = path.to_path_buf();
    if !path.is_absolute() {
        path = current_dir.join(&path);
    }

    Ok(path.clean())
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
        assert_eq!(
            dummy_desc
                .supervisor_path
                .to_str()
                .unwrap()
                .contains("dummy.exe"),
            true
        );

        let dummy_desc = registry.find_exeunit("wasm").unwrap();
        assert_eq!(dummy_desc.name.as_str(), "wasm");
        assert_eq!(
            dummy_desc
                .supervisor_path
                .to_str()
                .unwrap()
                .contains("wasm.exe"),
            true
        );
    }

    #[test]
    fn test_fill_registry_from_local_exe_unit_descriptor() {
        let exe_units_descriptor = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../exe-unit/resources/local-exeunits-descriptor.json");
        let mut registry = ExeUnitsRegistry::new();
        registry
            .register_exeunits_from_file(&exe_units_descriptor)
            .unwrap();

        let dummy_desc = registry.find_exeunit("wasmtime").unwrap();
        assert_eq!(dummy_desc.name.as_str(), "wasmtime");
        assert_eq!(
            dummy_desc
                .supervisor_path
                .to_str()
                .unwrap()
                .contains("exe-unit"),
            true
        );
    }

    #[test]
    fn test_fill_registry_from_deb_exe_unit_descriptor() {
        let exe_units_descriptor = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../exe-unit/resources/exeunits-descriptor.json");
        let mut registry = ExeUnitsRegistry::new();
        registry
            .register_exeunits_from_file(&exe_units_descriptor)
            .unwrap();

        let dummy_desc = registry.find_exeunit("wasmtime").unwrap();
        assert_eq!(dummy_desc.name.as_str(), "wasmtime");
        assert_eq!(
            dummy_desc
                .supervisor_path
                .to_str()
                .unwrap()
                .contains("/usr/lib/yagna/plugins/exe-unit"),
            true
        );
    }
}
