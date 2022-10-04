use crate::{
    command::UsageDef,
    utils::{get_command_json_output, move_string_out_of_json},
};
use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::command::YaCommand;

#[derive(Deserialize)]
pub struct ProviderConfig {
    pub node_name: String,
    pub subnet: Option<String>,
}

async fn show_provider_config(cmd: &YaCommand) -> Result<()> {
    let config = cmd.ya_provider()?.get_config().await?;
    println!("node name: {:?}", config.node_name.unwrap_or_default());
    Ok(())
}

#[derive(Deserialize)]
struct Resources {
    cpu_threads: i32,
    mem_gib: f64,
    storage_gib: f64,
}

async fn get_resources() -> Result<Resources> {
    let profiles = get_command_json_output("ya-provider", &["profile", "list", "--json"]).await?;
    let mut profiles = serde_json::from_value::<HashMap<String, Resources>>(profiles)?;

    let active_profile =
        get_command_json_output("ya-provider", &["profile", "active", "--json"]).await?;
    let active_profile =
        move_string_out_of_json(active_profile).ok_or_else(|| anyhow!("Invalid format"))?;

    Ok(profiles
        .remove(&active_profile)
        .ok_or_else(|| anyhow!("Active profile not found???"))?)
}

pub async fn show_resources() -> Result<()> {
    let resources = get_resources().await?;
    println!("Shared resources:");
    println!("\tcores:\t{}", resources.cpu_threads);
    println!("\tmemory:\t{} GiB", resources.mem_gib);
    println!("\tdisk:\t{} GiB", resources.storage_gib);
    Ok(())
}

async fn get_prices(cmd: &YaCommand) -> Result<BTreeMap<String, UsageDef>> {
    let presets = cmd.ya_provider()?.list_presets().await?;
    let active_presets: HashSet<String> = cmd
        .ya_provider()?
        .active_presets()
        .await?
        .into_iter()
        .collect();

    let mut usage_map = BTreeMap::new();
    if presets.is_empty() {
        bail!("No preset defined");
    }
    for p in presets {
        if active_presets.contains(&p.name) {
            let mut coeffs = p.usage_coeffs;
            coeffs.insert("initial".to_string(), p.initial_price);
            usage_map.insert(p.name, coeffs);
        }
    }
    Ok(usage_map)
}

pub async fn show_prices(cmd: &YaCommand) -> Result<()> {
    let price_description: HashMap<&str, (&str, f64)> = [
        ("golem.usage.cpu_sec", ("GLM per cpu hour", 3600.0)),
        ("initial", ("GLM for start", 1.0)),
        ("golem.usage.duration_sec", ("GLM per hour", 3600.0)),
        ("golem.usage.storage_gib", ("GLM per GB of storage", 1.0)),
    ]
    .iter()
    .cloned()
    .collect();
    let presets_prices = get_prices(cmd).await?;
    for (preset_name, prices) in presets_prices {
        println!("\n\nPricing for preset \"{}\":\n", preset_name);
        for (price_name, price_value) in prices {
            let default: (&str, f64) = (price_name.as_str(), 1.0);
            let (price_desc, price_multiplier) = price_description
                .get(&price_name.as_str())
                .unwrap_or(&default);
            println!("\t{:.18} {}", price_value * price_multiplier, price_desc);
        }
    }
    Ok(())
}

pub async fn run() -> Result</*exit code*/ i32> {
    let cmd = YaCommand::new()?;
    show_provider_config(&cmd).await?;
    show_resources().await?;
    show_prices(&cmd).await?;
    Ok(0)
}
