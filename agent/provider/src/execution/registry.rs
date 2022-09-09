use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{anyhow, Context, Result};
use futures::Future;
use path_clean::PathClean;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use ya_agreement_utils::OfferBuilder;

use super::exeunit_instance::ExeUnitInstance;

pub fn default_counter_config() -> HashMap<String, CounterDefinition> {
    let mut counters = HashMap::new();

    counters.insert(
        "golem.usage.duration_sec".into(),
        CounterDefinition {
            name: "duration".to_string(),
            description: "Duration".to_string(),
            price: true,
        },
    );
    counters.insert(
        "golem.usage.cpu_sec".into(),
        CounterDefinition {
            name: "cpu".to_string(),
            description: "CPU".to_string(),
            price: true,
        },
    );

    counters.insert(
        "golem.usage.storage_gib".into(),
        CounterDefinition {
            name: "storage_gib".into(),
            description: "Storage".into(),
            price: false,
        },
    );

    counters
}

/// Descriptor of ExeUnit
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct ExeUnitDesc {
    pub name: String,
    pub version: Version,
    #[serde(default)]
    pub description: Option<String>,
    pub supervisor_path: PathBuf,
    /// Additional arguments passed to supervisor.
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// Optional runtime.
    #[serde(default)]
    pub runtime_path: Option<PathBuf>,
    /// ExeUnit defined properties, that will be appended to offer.
    #[serde(default)]
    pub properties: Map<String, Value>,
    /// Here other capabilities and exe units metadata.
    pub config: Option<Configuration>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct Configuration {
    pub counters: HashMap<String, CounterDefinition>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct CounterDefinition {
    pub name: String,
    pub description: String,
    pub price: bool,
}

impl ExeUnitDesc {
    pub fn absolute_paths(self, base_path: &Path) -> std::io::Result<Self> {
        let mut desc = self;
        if desc.supervisor_path.is_relative() {
            desc.supervisor_path = base_path.join(&desc.supervisor_path);
        }
        if let Some(ref mut runtime_path) = &mut desc.runtime_path {
            if runtime_path.is_relative() {
                *runtime_path = base_path.join(runtime_path.as_path());
            }
        }
        Ok(desc)
    }

    pub fn resolve_coefficient(&self, coefficient_name: &str) -> Result<String> {
        self.config
            .as_ref()
            .and_then(|config| {
                if config.counters.contains_key(coefficient_name) {
                    return Some(coefficient_name.to_string());
                }
                config.counters.iter().find_map(|(prop_name, definition)| {
                    if definition.name.eq_ignore_ascii_case(coefficient_name) {
                        Some(prop_name.into())
                    } else {
                        None
                    }
                })
            })
            .ok_or_else(|| anyhow!("invalid coefficient name = {}", coefficient_name))
    }

    pub fn coefficient_name(&self, propery_name: &str) -> Option<String> {
        Some(
            self.config
                .as_ref()?
                .counters
                .get(propery_name)
                .as_ref()?
                .name
                .clone(),
        )
    }

    pub fn coefficients(&self) -> impl Iterator<Item = (String, CounterDefinition)> {
        if let Some(config) = &self.config {
            config.counters.clone().into_iter()
        } else {
            default_counter_config().into_iter()
        }
    }
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
        registry.register_exeunits_from_file(path)?;

        Ok(registry)
    }

    pub fn spawn_exeunit(
        &self,
        name: &str,
        args: Vec<String>,
        working_dir: &Path,
    ) -> Result<ExeUnitInstance> {
        let exeunit_desc = self.find_exeunit(name)?;
        let extended_args = Self::exeunit_args(&exeunit_desc, args)?;
        ExeUnitInstance::new(
            name,
            &exeunit_desc.supervisor_path,
            working_dir,
            &extended_args,
        )
    }

    pub fn run_exeunit_with_output(
        &self,
        name: &str,
        args: Vec<String>,
        working_dir: &Path,
    ) -> impl Future<Output = Result<String>> {
        let working_dir = working_dir.to_owned();
        let exeunit_desc = self.find_exeunit(name);
        async move {
            let exeunit_desc = exeunit_desc?;
            ExeUnitInstance::run_with_output(
                &exeunit_desc.supervisor_path,
                &working_dir,
                Self::exeunit_args(&exeunit_desc, args)?,
            )
            .await
        }
    }

    fn exeunit_args(exeunit_desc: &ExeUnitDesc, args: Vec<String>) -> Result<Vec<String>> {
        // Add to arguments path to runtime, which should be spawned
        // by supervisor as subprocess.
        let mut extended_args = Vec::new();
        if let Some(runtime_path) = &exeunit_desc.runtime_path {
            let runtime_path = runtime_path.to_str().ok_or_else(|| {
                anyhow!(
                    "ExeUnit runtime path [{}] contains invalid characters.",
                    runtime_path.display()
                )
            })?;
            extended_args.push("-b".to_owned());
            extended_args.push(runtime_path.to_owned());
        }
        // Add arguments from front. ExeUnit api requires positional
        // arguments, so ExeUnit can add only non-positional args.
        extended_args.extend(exeunit_desc.extra_args.iter().cloned());
        extended_args.extend(args);
        Ok(extended_args)
    }

    fn register_exeunit(&mut self, mut desc: ExeUnitDesc) -> Result<()> {
        desc.supervisor_path = normalize_path(&desc.supervisor_path)?;
        desc.runtime_path = if let Some(runtime_path) = &desc.runtime_path {
            Some(normalize_path(runtime_path)?)
        } else {
            None
        };

        log::info!(
            "Added [{}] ExeUnit to registry. Supervisor path: [{}], Runtime path: [{:?}].",
            desc.name,
            desc.supervisor_path.display(),
            desc.runtime_path
        );
        self.descriptors.insert(desc.name.clone(), desc);
        Ok(())
    }

    pub fn register_from_file_pattern(&mut self, pattern: &Path) -> Result<()> {
        log::debug!("Loading ExeUnit-s from: {}", pattern.display());

        for file in expand_filename(pattern)? {
            self.register_exeunits_from_file(&file)?;
        }
        Ok(())
    }

    pub fn register_exeunits_from_file(&mut self, path: &Path) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let base_path = path.parent().unwrap_or(&current_dir);
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

        for mut desc in descs.into_iter() {
            if desc.config.is_none() {
                desc.config = Some(Configuration {
                    counters: default_counter_config(),
                });
            }
            self.register_exeunit(desc.absolute_paths(base_path)?)?
        }
        Ok(())
    }

    pub fn find_exeunit(&self, name: &str) -> Result<ExeUnitDesc> {
        Ok(self
            .descriptors
            .get(name)
            .ok_or_else(|| anyhow!("ExeUnit [{}] doesn't exist in registry.", name))?
            .clone())
    }

    pub fn list(&self) -> Vec<ExeUnitDesc> {
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
        Err(RegistryError(errors))
    }

    pub fn test_runtimes(&self) -> anyhow::Result<()> {
        if self.descriptors.is_empty() {
            anyhow::bail!("No runtimes available");
        }

        for (name, desc) in self.descriptors.iter() {
            log::info!("Testing runtime [{}]", name);

            desc.runtime_path
                .as_ref()
                .map(|p| test_runtime(p))
                .unwrap_or(Ok(()))
                .map_err(|e| e.context("runtime test failure"))?;
        }

        Ok(())
    }
}

#[derive(Error, Debug)]
pub struct RegistryError(Vec<ExeUnitValidation>);

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for v in &self.0 {
            writeln!(f, "{}", v)?;
        }
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum ExeUnitValidation {
    #[error("ExeUnit [{}] Supervisor binary [{}] doesn't exist.", .desc.name, .desc.supervisor_path.display())]
    SupervisorNotFound { desc: ExeUnitDesc },
    #[error("ExeUnit [{}] Runtime binary [{:?}] doesn't exist.", .desc.name, .desc.runtime_path)]
    RuntimeNotFound { desc: ExeUnitDesc },
}

impl ExeUnitDesc {
    pub fn validate(&self) -> Result<(), ExeUnitValidation> {
        if !self.supervisor_path.exists() {
            return Err(ExeUnitValidation::SupervisorNotFound { desc: self.clone() });
        }
        if let Some(runtime_path) = self.runtime_path.as_ref() {
            if !runtime_path.exists() {
                return Err(ExeUnitValidation::RuntimeNotFound { desc: self.clone() });
            }
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

        serde_json::Value::Object(offer_part)
    }
}

fn test_runtime(path: &Path) -> anyhow::Result<()> {
    let child = Command::new(path)
        .arg("test")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let mut message = String::from_utf8_lossy(&output.stderr).to_string();
        if message.is_empty() {
            message = String::from_utf8_lossy(&output.stdout).to_string();
        }
        if !message.contains("--help") {
            anyhow::bail!(message);
        }
    }

    Ok(())
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

        writeln!(f, "{:width$}{}", "Name:", self.name, width = align)?;
        writeln!(f, "{:width$}{}", "Version:", self.version, width = align)?;
        writeln!(
            f,
            "{:width$}{}",
            "Supervisor:",
            self.supervisor_path.display(),
            width = align
        )?;
        if let Some(rt) = &self.runtime_path {
            writeln!(f, "{:width$}{}", "Runtime:", rt.display(), width = align)?;
        }
        writeln!(
            f,
            "{:width$}{}",
            "Description:",
            self.description
                .as_ref()
                .map(AsRef::as_ref)
                .unwrap_or("No description"),
            width = align
        )?;

        if !self.properties.is_empty() {
            writeln!(f, "Properties:")?;
            for (key, value) in self.properties.iter() {
                writeln!(f, "    {:width$}{}", key, value, width = align_prop)?;
            }
        }
        Ok(())
    }
}

fn expand_filename(pattern: &Path) -> Result<impl IntoIterator<Item = PathBuf>> {
    use std::fs::read_dir;

    let path: &Path = pattern;
    let (base_dir, file_name) = match (path.parent(), path.file_name()) {
        (Some(base_dir), Some(file_name)) => (base_dir, file_name),
        _ => return Ok(vec![PathBuf::from(pattern)]),
    };
    let file_name = match file_name.to_str() {
        Some(f) => f,
        None => anyhow::bail!("Not utf-8 filename: {:?}", file_name),
    };

    if let Some(pos) = file_name.find('*') {
        let (prefix, suffix) = file_name.split_at(pos);
        let suffix = &suffix[1..];

        Ok(read_dir(base_dir)
            .with_context(|| {
                format!(
                    "Looking for ExeUnit descriptors in dir: {}",
                    base_dir.display()
                )
            })?
            .filter_map(|ent| {
                let ent = ent.ok()?;
                let os_file_name = ent.file_name();
                let file_name = os_file_name.to_str()?;
                if file_name.starts_with(prefix) && file_name.ends_with(suffix) {
                    Some(ent.path())
                } else {
                    None
                }
            })
            .collect())
    } else {
        Ok(vec![PathBuf::from(pattern)])
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
