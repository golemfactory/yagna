use anyhow::{anyhow, Result};
use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Display, Default)]
#[serde(rename_all = "kebab-case")]
#[display(
    fmt = "Name: {}\nExeUnit: {}\nPricing model: {}\nCoefficients: {:?}",
    name,
    exeunit_name,
    pricing_model,
    usage_coeffs
)]
/// Preset describing offer, that can be saved and loaded from disk.
pub struct Preset {
    pub name: String,
    pub exeunit_name: String,
    pub pricing_model: String,
    pub usage_coeffs: Vec<f64>,
}

/// Responsible for presets management.
pub struct Presets {
    presets: HashMap<String, Preset>,
}

impl Presets {
    pub fn new() -> Presets {
        Presets {
            presets: HashMap::new(),
        }
    }

    pub fn from_file(presets_file: &Path) -> Result<Presets> {
        let mut presets = Presets::new();
        presets.load_from_file(&presets_file)?;
        Ok(presets)
    }

    pub fn load_from_file(&mut self, presets_file: &Path) -> Result<&mut Presets> {
        let file = File::open(presets_file).map_err(|error| {
            anyhow!(
                "Can't load Presets from file {}, error: {}.",
                presets_file.display(),
                error
            )
        })?;

        let reader = BufReader::new(file);
        let presets: Vec<Preset> = serde_json::from_reader(reader).map_err(|error| {
            anyhow!(
                "Can't deserialize Presets from file {}, error: {}.",
                presets_file.display(),
                error
            )
        })?;

        presets
            .into_iter()
            .map(|preset| self.add_preset(preset))
            .collect::<Result<()>>()?;
        Ok(self)
    }

    pub fn save_to_file(&self, presets_file: &Path) -> Result<()> {
        let file = File::create(presets_file).map_err(|error| {
            anyhow!(
                "Can't create Presets from file {}, error: {}.",
                presets_file.display(),
                error
            )
        })?;
        serde_json::to_writer_pretty(BufWriter::new(file), &self.list()).map_err(|error| {
            anyhow!(
                "Failed to serialize presets to file [{}], error: {}",
                presets_file.display(),
                error
            )
        })?;
        Ok(())
    }

    pub fn add_preset(&mut self, preset: Preset) -> Result<()> {
        if self.presets.contains_key(&preset.name) {
            return Err(anyhow!("Preset name [{}] already exists.", &preset.name));
        }

        self.presets.insert(preset.name.clone(), preset);
        Ok(())
    }

    pub fn remove_preset(&mut self, name: &str) -> Result<()> {
        if !self.presets.contains_key(name) {
            return Err(anyhow!("Preset [{}] doesn't exists.", &name));
        }
        self.presets.remove(name);
        Ok(())
    }

    pub fn list(&self) -> Vec<Preset> {
        self.presets
            .iter()
            .map(|(_, preset)| preset.clone())
            .collect()
    }

    pub fn list_matching(&self, names: &Vec<String>) -> Result<Vec<Preset>> {
        names
            .iter()
            .map(|name| match self.presets.get(name) {
                Some(preset) => Ok(preset.clone()),
                None => Err(anyhow!("Can't find preset [{}].", name)),
            })
            .collect()
    }
}

impl Preset {
    /// List usage metrics names, that should be added to agreement
    /// as 'properties.golem.com.usage.vector'. We could store them in preset
    /// in the future, but now let's treat them as constants, because there's
    /// not so many of them.
    pub fn list_usage_metrics(&self) -> Vec<String> {
        vec!["golem.usage.duration_sec", "golem.usage.cpu_sec"]
            .into_iter()
            .map(ToString::to_string)
            .collect()
    }
}
