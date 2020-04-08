use anyhow::{Result, anyhow};
use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::fs::File;
use std::io::BufReader;


#[derive(Serialize, Deserialize, Clone, Display)]
#[serde(rename_all = "kebab-case")]
#[display(
    fmt = "Name: {}\nExeUnit: {}\nCoefficients: {:?}",
    name,
    exeunit_name,
    usage_coeffs,
)]
/// Preset describing offer, that can be saved and loaded from disk.
pub struct Preset {
    name: String,
    exeunit_name: String,
    usage_coeffs: Vec<f64>,
}

/// Responsible for presets management.
pub struct Presets {
    presets: HashMap<String, Preset>,
}

impl Presets {
    pub fn new() -> Presets {
        Presets{ presets: HashMap::new() }
    }

    pub fn load_from_file(&mut self, presets_file: &Path) -> Result<()> {
        let file = File::open(presets_file)
            .map_err(|error| {
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

        presets.into_iter()
            .for_each(|preset| {
                self.presets.insert(preset.name.clone(), preset);
            });
        Ok(())
    }

    pub fn list(&self) -> Vec<Preset> {
        self.presets.iter()
            .map(|(_, preset)| preset.clone())
            .collect()
    }
}
