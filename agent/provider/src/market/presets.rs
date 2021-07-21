use std::collections::HashMap;
use std::fmt;
use std::fmt::Formatter;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

pub use crate::config::presets::Presets;
use crate::events::Event;
use crate::execution::ExeUnitsRegistry;
use crate::startup_config::FileMonitor;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
/// Preset describing offer, that can be saved and loaded from disk.
pub struct Preset {
    pub name: String,
    pub exeunit_name: String,
    pub pricing_model: String,
    pub initial_price: f64,
    pub usage_coeffs: HashMap<String, f64>,
}

impl Preset {
    pub fn get_initial_price(&self) -> Option<f64> {
        Some(self.initial_price)
    }

    pub fn display<'a, 'b>(&'a self, registry: &'b ExeUnitsRegistry) -> PresetDisplay<'a, 'b> {
        PresetDisplay {
            preset: self,
            registry,
        }
    }
}

/// Responsible for presets management.
pub struct PresetManager {
    pub(crate) state: Arc<Mutex<Presets>>,
    monitor: Option<FileMonitor>,
    sender: Option<watch::Sender<Event>>,
    receiver: watch::Receiver<Event>,
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
        let usage_coeffs = Default::default();

        Preset {
            name: "default".to_string(),
            initial_price: 0.0,
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

pub struct PresetDisplay<'a, 'b> {
    preset: &'a Preset,
    registry: &'b ExeUnitsRegistry,
}

impl<'a, 'b> fmt::Display for PresetDisplay<'a, 'b> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        display_preset(f, self.preset, self.registry)
    }
}

fn display_preset(
    f: &mut fmt::Formatter,
    preset: &Preset,
    registry: &ExeUnitsRegistry,
) -> fmt::Result {
    let align = 20;
    let align_coeff = align - 4; // Minus indent.

    write!(f, "{:width$}{}\n", "Name:", preset.name, width = align)?;
    write!(
        f,
        "{:width$}{}\n",
        "ExeUnit:",
        preset.exeunit_name,
        width = align
    )?;
    write!(
        f,
        "{:width$}{}\n",
        "Pricing model:",
        preset.pricing_model,
        width = align
    )?;
    write!(f, "{}\n", "Coefficients:")?;

    let exe_unit = registry.find_exeunit(&preset.exeunit_name).ok();

    for (name, coeff) in preset.usage_coeffs.iter() {
        let price_desc = exe_unit
            .as_ref()
            .and_then(|e| e.coefficient_name(&name))
            .unwrap_or_else(|| name.to_string());
        write!(
            f,
            "    {:width$}{} GLM\n",
            price_desc,
            coeff,
            width = align_coeff
        )?;
    }

    Ok(())
}
