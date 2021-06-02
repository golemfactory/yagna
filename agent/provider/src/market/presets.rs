use crate::events::Event;
use crate::startup_config::FileMonitor;
use anyhow::{anyhow, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;
use tokio::sync::watch;
use ya_utils_path::SwapSave;

#[derive(Serialize, Deserialize, Clone, Debug, Hash, Eq, PartialEq, EnumIter)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Coefficient {
    Duration,
    Cpu,
    Initial,
    HashUnit
}

impl Coefficient {
    #[inline]
    pub fn variants() -> impl Iterator<Item = Coefficient> {
        Self::iter()
    }

    pub fn to_property(&self) -> Option<&str> {
        let property = match self {
            Coefficient::Duration => "golem.usage.duration_sec",
            Coefficient::Cpu => "golem.usage.cpu_sec",
            Coefficient::HashUnit => "golem.usage.ethminer.hash",
            Coefficient::Initial => return None,
        };
        Some(property)
    }

    pub fn to_readable(&self) -> &str {
        match self {
            Coefficient::Duration => "Duration",
            Coefficient::Cpu => "CPU",
            Coefficient::Initial => "Init price",
            Coefficient::HashUnit => "Hash",
        }
    }
}

impl<'s> TryFrom<&'s str> for Coefficient {
    type Error = Error;

    fn try_from(value: &'s str) -> Result<Self> {
        match value {
            "Init price" | "initial" => Ok(Coefficient::Initial),
            "Duration" | "duration" => Ok(Coefficient::Duration),
            "CPU" | "cpu" => Ok(Coefficient::Cpu),
            "Hash" | "hash-unit" => Ok(Coefficient::HashUnit),
            _ => Err(anyhow!("Invalid coefficient: {}", value)),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
/// Preset describing offer, that can be saved and loaded from disk.
pub struct Preset {
    pub name: String,
    pub exeunit_name: String,
    pub pricing_model: String,
    pub usage_coeffs: HashMap<Coefficient, f64>,
}

impl Preset {
    pub fn get_initial_price(&self) -> Option<f64> {
        self.usage_coeffs.get(&Coefficient::Initial).cloned()
    }
}

/// Responsible for presets management.
pub struct PresetManager {
    pub(crate) state: Arc<Mutex<Presets>>,
    monitor: Option<FileMonitor>,
    sender: Option<watch::Sender<Event>>,
    receiver: watch::Receiver<Event>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct Presets {
    pub active: Vec<String>,
    pub presets: HashMap<String, Preset>,
}

impl Presets {
    pub fn load_from_file<P: AsRef<Path>>(presets_file: P) -> Result<Presets> {
        let path = presets_file.as_ref();
        log::debug!("Loading presets from: {}", path.display());
        let json = std::fs::read_to_string(path)?;
        let presets: Presets = serde_json::from_str::<PresetsFile>(json.as_str())
            .map_err(|e| anyhow!("Can't deserialize Presets from file {:?}: {}", path, e))?
            .into();

        match presets.active.is_empty() {
            false => presets.active.iter().try_for_each(|name| {
                presets
                    .presets
                    .get(name)
                    .ok_or(anyhow!("Invalid active preset: {:?}", name))
                    .map(|_| ())
            })?,
            _ => return Err(anyhow!("No active presets")),
        }

        Ok(presets)
    }

    pub fn save_to_file(&self, presets_file: &Path) -> Result<()> {
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

// FIXME: drop Preset::name so PresetsState can be serialized without conversion
#[derive(Serialize, Deserialize, Debug)]
struct PresetsFile {
    active: Vec<String>,
    presets: Vec<Preset>,
}

impl From<PresetsFile> for Presets {
    fn from(presets_file: PresetsFile) -> Self {
        Presets {
            active: presets_file.active,
            presets: presets_file
                .presets
                .into_iter()
                .map(|p| (p.name.clone(), p))
                .collect(),
        }
    }
}

impl<'p> From<&'p Presets> for PresetsFile {
    fn from(presets: &'p Presets) -> Self {
        PresetsFile {
            active: presets.active.clone(),
            presets: presets.presets.values().cloned().collect(),
        }
    }
}

impl PresetManager {
    pub fn new() -> PresetManager {
        let (sender, receiver) = watch::channel(Event::Initialized);
        PresetManager {
            state: Arc::new(Mutex::new(Presets::default())),
            monitor: None,
            sender: Some(sender),
            receiver,
        }
    }

    pub fn spawn_monitor<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let tx = self.sender.take().unwrap();
        let state = self.state.clone();
        let handler = move |p| match Presets::load_from_file(&p) {
            Ok(presets) => {
                let previous = { state.lock().unwrap().clone() };
                let (updated, removed) = previous.diff(&presets);
                let evt = Event::PresetsChanged {
                    presets,
                    updated,
                    removed,
                };
                tx.broadcast(evt).unwrap_or_default();
            }
            Err(e) => log::warn!("Error reading presets from {:?}: {:?}", p, e),
        };

        let monitor = FileMonitor::spawn(path, FileMonitor::on_modified(handler))?;
        self.monitor = Some(monitor);
        Ok(())
    }

    #[inline]
    pub fn event_receiver(&self) -> watch::Receiver<Event> {
        self.receiver.clone()
    }

    pub fn load_or_create(presets_file: &Path) -> Result<PresetManager> {
        if presets_file.exists() {
            Self::from_file(presets_file)
        } else {
            let presets = PresetManager::default();
            presets.save_to_file(presets_file)?;
            Ok(presets)
        }
    }

    pub fn from_file(presets_file: &Path) -> Result<PresetManager> {
        let presets = Presets::load_from_file(presets_file)?;
        let manager = PresetManager::new();
        {
            let mut state = manager.state.lock().unwrap();
            *state = presets;
        }
        Ok(manager)
    }

    pub fn save_to_file(&self, presets_file: &Path) -> Result<()> {
        let state = self.state.lock().unwrap();
        state.save_to_file(presets_file)
    }

    pub fn add_preset(&mut self, preset: Preset) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if state.presets.contains_key(&preset.name) {
            return Err(anyhow!("Preset name [{}] already exists.", &preset.name));
        }

        state.presets.insert(preset.name.clone(), preset);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<Preset> {
        let state = self.state.lock().unwrap();
        match state.presets.get(name) {
            Some(preset) => Ok(preset.clone()),
            None => Err(anyhow!("Preset [{}] doesn't exists.", &name)),
        }
    }

    pub fn remove_preset(&mut self, name: &str) -> Result<()> {
        let _ = self.deactivate(&name.to_string());
        let mut state = self.state.lock().unwrap();
        state
            .presets
            .remove(name)
            .ok_or(anyhow!("Preset [{}] doesn't exists.", name))?;

        Ok(())
    }

    pub fn update_preset<F>(&mut self, name: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut Preset) -> Result<()>,
    {
        let mut state = self.state.lock().unwrap();
        match state.presets.get_mut(name) {
            None => Err(anyhow!("Preset [{}] doesn't exists.", &name)),
            Some(preset) => {
                // if f fails, preset stays unchanged
                let mut new_preset = preset.clone();
                f(&mut new_preset)?;
                *preset = new_preset;
                Ok(())
            }
        }
    }

    pub fn active(&self) -> Vec<String> {
        let state = self.state.lock().unwrap();
        state.active.clone()
    }

    pub fn list(&self) -> Vec<Preset> {
        let state = self.state.lock().unwrap();
        state.presets.values().cloned().collect()
    }

    pub fn list_matching(&self, names: &Vec<String>) -> Result<Vec<Preset>> {
        let state = self.state.lock().unwrap();
        names
            .iter()
            .map(|name| match state.presets.get(name) {
                Some(preset) => Ok(preset.clone()),
                None => Err(anyhow!("Can't find preset [{}].", name)),
            })
            .collect()
    }

    pub fn list_names(&self) -> Vec<String> {
        let state = self.state.lock().unwrap();
        state.presets.keys().cloned().collect()
    }

    pub fn activate(&mut self, name: &String) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if !state.presets.contains_key(name) {
            return Err(anyhow!("Unknown preset: {:?}", name));
        }
        if !state.active.contains(name) {
            state.active.push(name.clone());
        }
        Ok(())
    }

    pub fn deactivate(&mut self, name: &String) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(idx) = state.active.iter().position(|n| name == n) {
            if state.active.len() == 1 {
                return Err(anyhow!("Cannot remove the last active preset: {:?}", name));
            }
            state.active.remove(idx);
            return Ok(());
        }
        Err(anyhow!("Preset not active: {:?}", name))
    }
}

impl Default for PresetManager {
    fn default() -> Self {
        let default = Preset::default();
        let mut presets = PresetManager::new();
        {
            let mut state = presets.state.lock().unwrap();
            state.active.push(default.name.clone());
        }
        presets.add_preset(default).unwrap();
        presets
    }
}

impl Default for Preset {
    // FIXME: sane defaults
    fn default() -> Self {
        let usage_coeffs = vec![
            (Coefficient::Duration, 0.1),
            (Coefficient::Cpu, 1.0),
            (Coefficient::Initial, 1.0),
        ]
        .into_iter()
        .collect();

        Preset {
            name: "default".to_string(),
            exeunit_name: "wasmtime".to_string(),
            pricing_model: "linear".to_string(),
            usage_coeffs,
        }
    }
}

impl PartialEq for Preset {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.exeunit_name == other.exeunit_name
            && self.pricing_model == other.pricing_model
            && self.usage_coeffs == other.usage_coeffs
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let align = 20;
        let align_coeff = align - 4; // Minus indent.

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

        for (name, coeff) in self.usage_coeffs.iter() {
            write!(
                f,
                "    {:width$}{} GLM\n",
                name.to_readable(),
                coeff,
                width = align_coeff
            )?;
        }

        Ok(())
    }
}
