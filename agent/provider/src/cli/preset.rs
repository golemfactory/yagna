use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use dialoguer::{Input, Select};
use structopt::StructOpt;

use crate::market::{Preset, PresetManager};
use crate::startup_config::{PresetNoInteractive, ProviderConfig, UpdateNames};

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub enum PresetsConfig {
    /// List available presets
    List,
    /// List active presets
    Active,
    /// Create a preset
    Create {
        #[structopt(long)]
        no_interactive: bool,
        #[structopt(flatten)]
        params: PresetNoInteractive,
    },
    /// Remove a preset
    Remove { name: String },
    /// Update a preset
    Update {
        #[structopt(flatten)]
        names: UpdateNames,
        #[structopt(long)]
        no_interactive: bool,
        #[structopt(flatten)]
        params: PresetNoInteractive,
    },
    /// Activate a preset
    Activate { name: String },
    /// Deactivate a preset
    Deactivate { name: String },
}

impl PresetsConfig {
    pub fn run(self, config: ProviderConfig) -> anyhow::Result<()> {
        match self {
            PresetsConfig::List => list(config),
            PresetsConfig::Active => active_presets(config),
            PresetsConfig::Create {
                no_interactive,
                params,
            } => {
                if no_interactive {
                    create(config, params)
                } else {
                    create_interactive(config)
                }
            }
            PresetsConfig::Remove { name } => remove_preset(config, name),
            PresetsConfig::Update {
                no_interactive,
                params,
                mut names,
            } => {
                if no_interactive {
                    update_presets(&config, names, params)
                } else {
                    if names.all || names.names.len() != 1 {
                        anyhow::bail!("choose one name for interactive update");
                    }
                    update_preset_interactive(config, names.names.drain(..).next().unwrap())
                }
            }
            PresetsConfig::Activate { name } => activate_preset(config, name),
            PresetsConfig::Deactivate { name } => deactivate_preset(config, name),
        }
    }
}

pub struct PresetUpdater {
    preset: Preset,
    exeunits: Vec<String>,
    pricing_models: Vec<String>,
}

impl PresetUpdater {
    pub fn new(
        preset: Preset,
        exeunits: Vec<String>,
        pricing_models: Vec<String>,
    ) -> PresetUpdater {
        PresetUpdater {
            preset,
            exeunits,
            pricing_models,
        }
    }

    pub fn update_exeunit(&mut self) -> Result<()> {
        let prev_exeunit = self
            .exeunits
            .iter()
            .position(|exeunit| exeunit == &self.preset.exeunit_name)
            .unwrap_or(0);

        let exeunit_idx = Select::new()
            .with_prompt("ExeUnit")
            .items(&self.exeunits[..])
            .default(prev_exeunit)
            .interact()?;
        self.preset.exeunit_name = self.exeunits[exeunit_idx].clone();
        Ok(())
    }

    pub fn update_pricing_model(&mut self) -> Result<()> {
        let prev_pricing = self
            .pricing_models
            .iter()
            .position(|pricing| pricing == &self.preset.pricing_model)
            .unwrap_or(0);

        let pricing_idx = Select::new()
            .with_prompt("Pricing model")
            .items(&self.pricing_models[..])
            .default(prev_pricing)
            .interact()?;
        self.preset.pricing_model = self.pricing_models[pricing_idx].clone();
        Ok(())
    }

    pub fn update_metrics(&mut self, config: &ProviderConfig) -> Result<()> {
        let registry = config.registry()?;
        let mut usage_coeffs: HashMap<String, f64> = Default::default();
        let exe_unit_desc = registry.find_exeunit(&self.preset.exeunit_name)?;

        fn get_usage(m: &HashMap<String, f64>, k1: &str, k2: &str) -> f64 {
            m.get(k1)
                .cloned()
                .unwrap_or_else(|| m.get(k2).cloned().unwrap_or(0.))
        }

        for (prop_name, counter) in exe_unit_desc.coefficients() {
            if counter.price {
                let prev_price = get_usage(&self.preset.usage_coeffs, &prop_name, &counter.name);
                let price = Input::<f64>::new()
                    .with_prompt(&format!("{} (GLM)", &counter.description))
                    .default(prev_price)
                    .show_default(true)
                    .interact()?;
                usage_coeffs.insert(prop_name, price);
            }
        }

        self.preset.usage_coeffs = usage_coeffs;
        Ok(())
    }

    pub fn update_name(&mut self) -> Result<()> {
        self.preset.name = Input::<String>::new()
            .with_prompt("Preset name")
            .default(self.preset.name.clone())
            .show_default(true)
            .interact()?;
        Ok(())
    }

    pub fn interact(mut self, config: &ProviderConfig) -> Result<Preset> {
        self.update_name()?;
        self.update_exeunit()?;
        self.update_pricing_model()?;
        self.update_metrics(config)?;

        Ok(self.preset)
    }
}

pub fn create_interactive(config: ProviderConfig) -> anyhow::Result<()> {
    if config.json {
        anyhow::bail!("json output not implemented");
    }

    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    let registry = config.registry()?;

    let exeunits = registry.list().into_iter().map(|desc| desc.name).collect();
    let pricing_models = vec!["linear".to_string()];

    let preset =
        PresetUpdater::new(Preset::default(), exeunits, pricing_models).interact(&config)?;

    presets.add_preset(preset.clone())?;
    presets.save_to_file(&config.presets_file)?;

    println!();
    println!("Preset created:");
    println!("{}", preset.display(&registry));
    Ok(())
}

fn is_initial_coefficient_name(name : &str) -> bool {
    name.eq_ignore_ascii_case("initial") || name.eq("Init price")
}

pub fn create(config: ProviderConfig, params: PresetNoInteractive) -> anyhow::Result<()> {
    if config.json {
        anyhow::bail!("json output not implemented");
    }

    let mut presets = PresetManager::load_or_create(&config.presets_file)?;

    let mut preset = Preset::default();
    preset.name = params
        .preset_name
        .ok_or(anyhow!("Preset name is required."))?;
    preset.exeunit_name = params.exe_unit.ok_or(anyhow!("ExeUnit is required."))?;
    preset.pricing_model = params.pricing.unwrap_or("linear".to_string());

    let registry = config.registry()?;

    let exe_unit_desc = registry.find_exeunit(&preset.exeunit_name)?;

    for (name, price) in params.price.iter() {
        if is_initial_coefficient_name(name) {
            preset.initial_price = *price;
        }
        else {
            let usage_coefficient = exe_unit_desc.resolve_coefficient(&name)?;

            preset.usage_coeffs.insert(usage_coefficient, *price);
        }
    }

    validate_preset(&config, &preset)?;

    presets.add_preset(preset.clone())?;
    presets.save_to_file(&config.presets_file)?;

    println!();
    println!("Preset created:");
    println!("{}", preset.display(&registry));
    Ok(())
}

fn list(config: ProviderConfig) -> anyhow::Result<()> {
    let presets = PresetManager::load_or_create(&config.presets_file)?;
    let registry = config.registry()?;

    if config.json {
        println!("{}", serde_json::to_string_pretty(&presets.list())?);
    } else {
        println!("Available Presets:");

        for preset in presets.list().iter() {
            println!("\n{}", preset.display(&registry));
        }
    }
    Ok(())
}

fn active_presets(config: ProviderConfig) -> anyhow::Result<()> {
    let presets = PresetManager::load_or_create(&config.presets_file)?;
    if config.json {
        println!("{}", serde_json::to_string_pretty(&presets.active())?);
    } else {
        for preset in presets.active() {
            println!("\n{}", preset);
        }
    }
    Ok(())
}

fn activate_preset(config: ProviderConfig, name: String) -> anyhow::Result<()> {
    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    presets.activate(&name)?;
    presets.save_to_file(&config.presets_file)
}

fn deactivate_preset(config: ProviderConfig, name: String) -> anyhow::Result<()> {
    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    presets.deactivate(&name)?;
    presets.save_to_file(&config.presets_file)
}

fn update_presets(
    config: &ProviderConfig,
    names: UpdateNames,
    params: PresetNoInteractive,
) -> anyhow::Result<()> {
    if config.json {
        anyhow::bail!("json output not implemented");
    }

    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    let registry = config.registry()?;

    let names = if names.all {
        presets.list_names()
    } else {
        names.names
    };

    for name in names {
        let params = params.clone();
        presets.update_preset(&name, |preset| -> anyhow::Result<()> {
            if let Some(new_name) = params.preset_name {
                preset.name = new_name;
            }
            if let Some(new_exeunit_name) = params.exe_unit {
                preset.exeunit_name = new_exeunit_name;
            }
            if let Some(new_pricing_model) = params.pricing {
                preset.pricing_model = new_pricing_model;
            }
            let exe_unit_desc = registry.find_exeunit(&preset.exeunit_name)?;

            for (name, price) in params.price.iter() {
                if is_initial_coefficient_name(name) {
                    preset.initial_price = *price;
                } else {
                    preset
                        .usage_coeffs
                        .insert(exe_unit_desc.resolve_coefficient(&name)?, *price);
                }
            }

            validate_preset(&config, &preset)?;

            Ok(())
        })?;
    }

    presets.save_to_file(&config.presets_file)?;

    println!();
    println!("Presets updated");
    Ok(())
}

fn remove_preset(config: ProviderConfig, name: String) -> anyhow::Result<()> {
    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    presets.remove_preset(&name)?;
    presets.save_to_file(&config.presets_file)
}

fn validate_preset(config: &ProviderConfig, preset: &Preset) -> anyhow::Result<()> {
    // Validate ExeUnit existence and pricing model.
    let registry = config.registry()?;
    registry.find_exeunit(&preset.exeunit_name)?;

    if !(preset.pricing_model == "linear") {
        bail!("Not supported pricing model.")
    }

    Ok(())
}

fn update_preset_interactive(config: ProviderConfig, name: String) -> anyhow::Result<()> {
    if config.json {
        anyhow::bail!("json output not implemented");
    }

    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    let registry = config.registry()?;

    let exeunits = registry.list().into_iter().map(|desc| desc.name).collect();
    let pricing_models = vec!["linear".to_string()];

    let preset =
        PresetUpdater::new(presets.get(&name)?, exeunits, pricing_models).interact(&config)?;

    presets.remove_preset(&name)?;
    presets.add_preset(preset.clone())?;
    presets.save_to_file(&config.presets_file)?;

    println!();
    println!("Preset updated:");
    println!("{}", preset.display(&registry));
    Ok(())
}
