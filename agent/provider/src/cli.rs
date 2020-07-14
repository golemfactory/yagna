use crate::market::presets::Coefficient;
use crate::market::{Preset, PresetManager};
use crate::preset_cli::PresetUpdater;
use crate::startup_config::{PresetNoInteractive, ProviderConfig};
use anyhow::{anyhow, bail};
use std::convert::TryFrom;

pub fn list_exeunits(config: ProviderConfig) -> anyhow::Result<()> {
    let registry = config.registry()?;
    if let Err(errors) = registry.validate() {
        println!("Encountered errors while checking ExeUnits:\n{}", errors);
    }

    println!("Available ExeUnits:");

    let exeunits = registry.list_exeunits();
    for exeunit in exeunits.iter() {
        println!("\n{}", exeunit);
    }
    Ok(())
}

pub fn list_presets(config: ProviderConfig) -> anyhow::Result<()> {
    let presets = PresetManager::load_or_create(&config.presets_file)?;
    println!("Available Presets:");

    for preset in presets.list().iter() {
        println!("\n{}", preset);
    }
    Ok(())
}

pub fn list_metrics(_: ProviderConfig) -> anyhow::Result<()> {
    for entry in Coefficient::variants() {
        if let Some(property) = entry.to_property() {
            println!("{:15}{}", entry, property);
        }
    }
    Ok(())
}

pub fn create_preset(config: ProviderConfig, params: PresetNoInteractive) -> anyhow::Result<()> {
    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    let registry = config.registry()?;

    let mut preset = Preset::default();
    preset.name = params
        .preset_name
        .ok_or(anyhow!("Preset name is required."))?;
    preset.exeunit_name = params.exe_unit.ok_or(anyhow!("ExeUnit is required."))?;
    preset.pricing_model = params.pricing.unwrap_or("linear".to_string());

    for (name, price) in params.price.iter() {
        preset
            .usage_coeffs
            .insert(Coefficient::try_from(name.as_str())?, *price);
    }

    // Validate ExeUnit existence and pricing model.
    registry.find_exeunit(&preset.exeunit_name)?;
    if !(preset.pricing_model == "linear") {
        bail!("Not supported pricing model.")
    }

    presets.add_preset(preset.clone())?;
    presets.save_to_file(&config.presets_file)?;

    println!();
    println!("Preset created:");
    println!("{}", preset);
    Ok(())
}

pub fn create_preset_interactive(config: ProviderConfig) -> anyhow::Result<()> {
    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    let registry = config.registry()?;

    let exeunits = registry
        .list_exeunits()
        .into_iter()
        .map(|desc| desc.name)
        .collect();
    let pricing_models = vec!["linear".to_string()];

    let preset = PresetUpdater::new(Preset::default(), exeunits, pricing_models).interact()?;

    presets.add_preset(preset.clone())?;
    presets.save_to_file(&config.presets_file)?;

    println!();
    println!("Preset created:");
    println!("{}", preset);
    Ok(())
}

pub fn remove_preset(config: ProviderConfig, name: String) -> anyhow::Result<()> {
    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    presets.remove_preset(&name)?;
    presets.save_to_file(&config.presets_file)
}

pub fn active_presets(config: ProviderConfig) -> anyhow::Result<()> {
    let presets = PresetManager::load_or_create(&config.presets_file)?;
    for preset in presets.active() {
        println!("\n{}", preset);
    }
    Ok(())
}

pub fn activate_preset(config: ProviderConfig, name: String) -> anyhow::Result<()> {
    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    presets.activate(&name)?;
    presets.save_to_file(&config.presets_file)
}

pub fn deactivate_preset(config: ProviderConfig, name: String) -> anyhow::Result<()> {
    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    presets.deactivate(&name)?;
    presets.save_to_file(&config.presets_file)
}

pub fn update_preset_interactive(config: ProviderConfig, name: String) -> anyhow::Result<()> {
    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    let registry = config.registry()?;

    let exeunits = registry
        .list_exeunits()
        .into_iter()
        .map(|desc| desc.name)
        .collect();
    let pricing_models = vec!["linear".to_string()];

    let preset = PresetUpdater::new(presets.get(&name)?, exeunits, pricing_models).interact()?;

    presets.remove_preset(&name)?;
    presets.add_preset(preset.clone())?;
    presets.save_to_file(&config.presets_file)?;

    println!();
    println!("Preset updated:");
    println!("{}", preset);
    Ok(())
}

pub fn update_preset(
    config: ProviderConfig,
    name: String,
    params: PresetNoInteractive,
) -> anyhow::Result<()> {
    let mut presets = PresetManager::load_or_create(&config.presets_file)?;
    let registry = config.registry()?;

    let mut preset = presets.get(&name)?;

    // All values are optional. If not set, previous value will remain.
    preset.name = params.preset_name.unwrap_or(preset.name);
    preset.exeunit_name = params.exe_unit.unwrap_or(preset.exeunit_name);
    preset.pricing_model = params.pricing.unwrap_or(preset.pricing_model);

    for (name, price) in params.price.iter() {
        preset
            .usage_coeffs
            .insert(Coefficient::try_from(name.as_str())?, *price);
    }

    // Validate ExeUnit existence and pricing model.
    registry.find_exeunit(&preset.exeunit_name)?;
    if !(preset.pricing_model == "linear") {
        bail!("Not supported pricing model.")
    }

    presets.remove_preset(&name)?;
    presets.add_preset(preset.clone())?;
    presets.save_to_file(&config.presets_file)?;

    println!();
    println!("Preset updated:");
    println!("{}", preset);
    Ok(())
}
