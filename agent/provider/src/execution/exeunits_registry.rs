use anyhow::{anyhow, Result};
use path_clean::PathClean;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};
use thiserror::Error;

use super::exeunit_instance::ExeUnitInstance;
use serde_json::Value;
use ya_agent_offer_model::OfferBuilder;

/// Descriptor of ExeUnit
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct ExeUnitDesc {
    pub name: String,
    pub version: Version,
    pub supervisor_path: PathBuf,
    pub runtime_path: PathBuf,

    // ExeUnit defined properties, that will be appended to offer.
    #[serde(default = "empty_properties")]
    pub properties: serde_json::Map<String, Value>,

    // Here other capabilities and exe units metadata.
    #[serde(default = "default_description")]
    pub description: String,
}

fn default_description() -> String {
    "No description provided.".to_string()
}

fn empty_properties() -> serde_json::Map<String, Value> {
    serde_json::Map::<String, Value>::new()
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

    pub fn from_file(path: &Path) -> Result<ExeUnitsRegistry> {
        let mut registry = ExeUnitsRegistry::new();
        registry.register_exeunits_from_file(&path)?;

        Ok(registry)
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

        log::info!(
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

    pub fn list_exeunits(&self) -> Vec<ExeUnitDesc> {
        self.descriptors
            .iter()
            .map(|(_, desc)| desc.clone())
            .collect()
    }

    pub fn validate(&self) -> Result<(), RegistryError> {
        let errors = self
            .descriptors
            .iter()
            .map(|(_, desc)| desc.validate())
            .filter_map(|result| match result {
                Err(error) => Some(error),
                Ok(_) => None,
            })
            .collect::<Vec<ExeUnitValidation>>();

        if errors.is_empty() {
            return Ok(());
        }
        return Err(RegistryError(errors));
    }
}

#[derive(Error, Debug)]
pub struct RegistryError(Vec<ExeUnitValidation>);

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for v in &self.0 {
            write!(f, "{}\n", v)?;
        }
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum ExeUnitValidation {
    #[error("ExeUnit [{}] Supervisor binary [{}] doesn't exist.", .desc.name, .desc.supervisor_path.display())]
    SupervisorNotFound { desc: ExeUnitDesc },
    #[error("ExeUnit [{}] Runtime binary [{}] doesn't exist.", .desc.name, .desc.supervisor_path.display())]
    RuntimeNotFound { desc: ExeUnitDesc },
}

impl ExeUnitDesc {
    pub fn validate(&self) -> Result<(), ExeUnitValidation> {
        if !self.supervisor_path.exists() {
            return Err(ExeUnitValidation::SupervisorNotFound { desc: self.clone() });
        }
        // I'm not sure we should force it's existence.
        if !self.runtime_path.exists() {
            return Err(ExeUnitValidation::RuntimeNotFound { desc: self.clone() });
        }
        Ok(())
    }
}

impl OfferBuilder for ExeUnitDesc {
    fn build(&self) -> Value {
        let mut common = serde_json::json!({
            "name": self.name,
            "version": self.version.to_string()
        });
        let mut offer_part = self.properties.clone();
        offer_part.append(common.as_object_mut().unwrap());

        return serde_json::Value::Object(offer_part);
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

impl fmt::Display for ExeUnitDesc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let align = 15;
        let align_prop = 30;

        write!(f, "{:width$}{}\n", "Name:", self.name, width = align)?;
        write!(f, "{:width$}{}\n", "Version:", self.version, width = align)?;
        write!(
            f,
            "{:width$}{}\n",
            "Supervisor:",
            self.supervisor_path.display(),
            width = align
        )?;
        write!(
            f,
            "{:width$}{}\n",
            "Runtime:",
            self.runtime_path.display(),
            width = align
        )?;
        write!(
            f,
            "{:width$}{}\n",
            "Description:",
            self.description,
            width = align
        )?;

        if !self.properties.is_empty() {
            write!(f, "Properties:\n")?;
            for (key, value) in self.properties.iter() {
                write!(f, "    {:width$}{}\n", key, value, width = align_prop)?;
            }
        }
        Ok(())
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
