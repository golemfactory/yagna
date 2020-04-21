use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
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
        let file = match File::open(presets_file) {
            Ok(file) => file,
            Err(_) => {
                self.save_to_file(presets_file)?;
                File::open(presets_file)?
            }
        };

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

    pub fn get(&self, name: &str) -> Result<Preset> {
        match self.presets.get(name) {
            Some(preset) => Ok(preset.clone()),
            None => Err(anyhow!("Preset [{}] doesn't exists.", &name)),
        }
    }

    pub fn remove_preset(&mut self, name: &str) -> Result<()> {
        self.presets
            .remove(name)
            .ok_or(anyhow!("Preset [{}] doesn't exists.", name))?;
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

    pub fn list_readable_metrics(&self) -> Vec<String> {
        vec!["Duration", "CPU"]
            .into_iter()
            .map(ToString::to_string)
            .collect()
    }

    pub fn update_price(&mut self, metric: &str, price: f64) -> Result<()> {
        let idx = if metric == "Init price" {
            self.list_readable_metrics().len()
        } else {
            self.list_readable_metrics()
                .iter()
                .position(|name| name == metric)
                .ok_or(anyhow!("Metric [{}] not found.", metric))?
        };
        self.usage_coeffs[idx] = price;
        Ok(())
    }
}

impl Default for Preset {
    fn default() -> Self {
        Preset {
            name: "".to_string(),
            exeunit_name: "".to_string(),
            pricing_model: "".to_string(),
            usage_coeffs: vec![0.0, 0.0, 0.0],
        }
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let align = 20;
        let align_coeff = align - 4; // Minus intent.

        write!(f, "{:width$}{}\n", "Name:", self.name, width = align)?;
        write!(
            f,
            "{:width$}{}\n",
            "ExeUnit:",
            self.exeunit_name,
            width = align
        )?;
        write!(
            f,
            "{:width$}{}\n",
            "Pricing model:",
            self.pricing_model,
            width = align
        )?;
        write!(f, "{}\n", "Coefficients:")?;

        for (coeff, name) in self
            .usage_coeffs
            .iter()
            .zip(self.list_readable_metrics().iter())
        {
            write!(f, "    {:width$}{} GNT\n", name, coeff, width = align_coeff)?;
        }

        write!(
            f,
            "    {:16}{} GNT",
            "Init price",
            self.usage_coeffs[self.usage_coeffs.len() - 1]
        )?;
        Ok(())
    }
}
