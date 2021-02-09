use crate::hardware::{ProfileError, Profiles, Resources, UpdateResources};
use crate::market::presets::Coefficient;
use crate::market::{Preset, PresetManager};
use crate::preset_cli::PresetUpdater;
use crate::provider_agent;
use crate::startup_config::{PresetNoInteractive, ProviderConfig, UpdateNames};
use anyhow::{anyhow, bail};
use std::convert::TryFrom;

pub fn config_get(config: ProviderConfig, name: Option<String>) -> anyhow::Result<()> {
    let globals_state = provider_agent::GlobalsState::load(&config.globals_file)?;
    match name {
        None => {
            if config.json {
                println!("{}", serde_json::to_string_pretty(&globals_state)?);
            } else {
                println!("{}", &globals_state)
            }
        }
        Some(name) => {
            let state = serde_json::to_value(globals_state)?;
            let value = state
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("Invalid name global state property: {}", name))?;
            if config.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!("{}: {}", name, serde_json::to_string_pretty(value)?);
            }
        }
    }
    Ok(())
}

pub fn list_exeunits(config: ProviderConfig) -> anyhow::Result<()> {
    let registry = config.registry()?;
    if let Err(errors) = registry.validate() {
        eprintln!("Encountered errors while checking ExeUnits:\n{}", errors);
    }

    if config.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&registry.list_exeunits())?
        );
    } else {
        println!("Available ExeUnits:");

        let exeunits = registry.list_exeunits();
        for exeunit in exeunits.iter() {
            println!("\n{}", exeunit);
        }
    }
    Ok(())
}

pub fn list_presets(config: ProviderConfig) -> anyhow::Result<()> {
    let presets = PresetManager::load_or_create(&config.presets_file)?;

    if config.json {
        println!("{}", serde_json::to_string_pretty(&presets.list())?);
    } else {
        println!("Available Presets:");

        for preset in presets.list().iter() {
            println!("\n{}", preset);
        }
    }
    Ok(())
}

pub fn list_metrics(config: ProviderConfig) -> anyhow::Result<()> {
    if config.json {
        anyhow::bail!("json output not implemented");
    }

    for entry in Coefficient::variants() {
        if let Some(property) = entry.to_property() {
            println!("{:15}{}", entry.to_readable(), property);
        }
    }
    Ok(())
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

pub fn create_preset(config: ProviderConfig, params: PresetNoInteractive) -> anyhow::Result<()> {
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

    for (name, price) in params.price.iter() {
        preset
            .usage_coeffs
            .insert(Coefficient::try_from(name.as_str())?, *price);
    }

    validate_preset(&config, &preset)?;

    presets.add_preset(preset.clone())?;
    presets.save_to_file(&config.presets_file)?;

    println!();
    println!("Preset created:");
    println!("{}", preset);
    Ok(())
}

pub fn create_preset_interactive(config: ProviderConfig) -> anyhow::Result<()> {
    if config.json {
        anyhow::bail!("json output not implemented");
    }

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
    if config.json {
        println!("{}", serde_json::to_string_pretty(&presets.active())?);
    } else {
        for preset in presets.active() {
            println!("\n{}", preset);
        }
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
    if config.json {
        anyhow::bail!("json output not implemented");
    }

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

pub fn update_presets(
    config: &ProviderConfig,
    names: UpdateNames,
    params: PresetNoInteractive,
) -> anyhow::Result<()> {
    if config.json {
        anyhow::bail!("json output not implemented");
    }

    let mut presets = PresetManager::load_or_create(&config.presets_file)?;

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

            for (name, price) in params.price.iter() {
                preset
                    .usage_coeffs
                    .insert(Coefficient::try_from(name.as_str())?, *price);
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

fn update_profile(resources: &mut Resources, new_resources: UpdateResources) {
    if let Some(cpu_threads) = new_resources.cpu_threads {
        resources.cpu_threads = cpu_threads;
    }
    if let Some(mem_gib) = new_resources.mem_gib {
        resources.mem_gib = mem_gib;
    }
    if let Some(storage_gib) = new_resources.storage_gib {
        resources.storage_gib = storage_gib;
    }
}

pub fn update_profiles(
    config: ProviderConfig,
    names: UpdateNames,
    new_resources: UpdateResources,
) -> anyhow::Result<()> {
    let mut profiles = Profiles::load_or_create(&config)?;

    if names.all {
        for resources in profiles.list().values_mut() {
            update_profile(resources, new_resources);
        }
    } else {
        for name in names.names {
            match profiles.get_mut(&name) {
                Some(resources) => update_profile(resources, new_resources),
                _ => return Err(ProfileError::Unknown(name).into()),
            }
        }
    }

    profiles.save(config.hardware_file.as_path())?;
    Ok(())
}
