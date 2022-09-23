use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use ya_utils_path::SwapSave;

use crate::market::Preset;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PresetV0 {
    pub name: String,
    pub exeunit_name: String,
    pub pricing_model: String,
    pub usage_coeffs: HashMap<String, f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct Presets {
    pub active: Vec<String>,
    pub presets: HashMap<String, Preset>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "ver")]
enum PresetsFile {
    V0 {
        active: Vec<String>,
        presets: Vec<PresetV0>,
    },
    V1 {
        active: Vec<String>,
        presets: Vec<Preset>,
    },
}

impl Presets {
    pub fn load_from_file<P: AsRef<Path>>(presets_file: P) -> anyhow::Result<Presets> {
        let path = presets_file.as_ref();
        log::debug!("Loading presets from: {}", path.display());
        let json = std::fs::read_to_string(path)?;
        let mut val: serde_json::Value = serde_json::from_str(&json)?;
        if let Some(obj) = val.as_object_mut() {
            if !obj.contains_key("ver") {
                obj.insert("ver".into(), "V0".into());
            }
        }

        let presets: Presets = serde_json::from_value::<PresetsFile>(val)
            .map_err(|e| anyhow!("Can't deserialize Presets from file {:?}: {}", path, e))?
            .into();

        presets.active.iter().try_for_each(|name| {
            presets
                .presets
                .get(name)
                .ok_or_else(|| anyhow!("Invalid active preset: {:?}", name))
                .map(|_| ())
        })?;

        Ok(presets)
    }

    pub fn save_to_file(&self, presets_file: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&PresetsFile::from(self))
            .map_err(|error| anyhow!("Failed to serialize Presets: {}", error))?;
        presets_file.swap_save(json).map_err(|error| {
            anyhow!(
                "Failed to save Presets to file {}, error: {}.",
                presets_file.display(),
                error
            )
        })?;
        Ok(())
    }

    pub fn diff(&self, other: &Presets) -> (Vec<String>, Vec<String>) {
        let mut updated = HashSet::new();
        let mut removed = HashSet::new();

        self.active.iter().for_each(|n| {
            if !other.active.contains(n) {
                removed.insert(n.clone());
            }
        });
        self.presets
            .iter()
            .for_each(|(n, p)| match other.presets.get(n) {
                Some(preset) => {
                    if preset != p {
                        updated.insert(n.clone());
                    }
                }
                _ => {
                    removed.insert(n.clone());
                }
            });

        (updated.into_iter().collect(), removed.into_iter().collect())
    }
}

impl Default for Presets {
    fn default() -> Self {
        Presets {
            active: Vec::new(),
            presets: HashMap::new(),
        }
    }
}

impl From<PresetsFile> for Presets {
    fn from(presets_file: PresetsFile) -> Self {
        match presets_file {
            PresetsFile::V0 { active, presets } => Presets {
                active,
                presets: presets
                    .into_iter()
                    .map(|p: PresetV0| (p.name.clone(), p.into()))
                    .collect(),
            },
            PresetsFile::V1 { active, presets } => Presets {
                active,
                presets: presets.into_iter().map(|p| (p.name.clone(), p)).collect(),
            },
        }
    }
}

impl<'p> From<&'p Presets> for PresetsFile {
    fn from(presets: &'p Presets) -> Self {
        PresetsFile::V1 {
            active: presets.active.clone(),
            presets: presets.presets.values().cloned().collect(),
        }
    }
}

impl From<PresetV0> for Preset {
    fn from(old_preset: PresetV0) -> Self {
        Preset {
            name: old_preset.name,
            exeunit_name: old_preset.exeunit_name,
            pricing_model: old_preset.pricing_model,
            initial_price: old_preset
                .usage_coeffs
                .get("initial")
                .cloned()
                .unwrap_or(0f64),
            usage_coeffs: old_preset
                .usage_coeffs
                .into_iter()
                .filter_map(|(name, price)| match name.as_str() {
                    "duration" => Some(("golem.usage.duration_sec".to_string(), price)),
                    "cpu" => Some(("golem.usage.cpu_sec".to_string(), price)),
                    _ => None,
                })
                .collect(),
        }
    }
}
