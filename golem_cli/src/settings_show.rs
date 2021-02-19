use crate::utils::{get_command_json_output, move_string_out_of_json};
use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

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

#[derive(Copy, Clone, PartialEq, Deserialize)]
struct Prices {
    duration: f64,
    cpu: f64,
    initial: f64,
}

async fn get_prices(cmd: &YaCommand) -> Result<Prices> {
    let presets = cmd.ya_provider()?.list_presets().await?;
    let active_presets: HashSet<String> = cmd
        .ya_provider()?
        .active_presets()
        .await?
        .into_iter()
        .collect();

    if let Some(preset) = presets.iter().find(|&p| active_presets.contains(&p.name)) {
        return Ok(Prices {
            duration: preset.usage_coeffs.duration,
            cpu: preset.usage_coeffs.cpu,
            initial: preset.usage_coeffs.initial,
        });
    }
    bail!("No preset defined");
}

pub async fn show_prices(cmd: &YaCommand) -> Result<()> {
    let prices = get_prices(cmd).await?;
    println!("\n\nPricing:\n");
    println!("\t{:5} GLM for start", prices.initial);
    println!("\t{:5} GLM per hour", prices.duration * 3600.0);
    println!("\t{:5} GLM per cpu hour", prices.cpu * 3600.0);
    Ok(())
}

pub async fn run() -> Result</*exit code*/ i32> {
    let cmd = YaCommand::new()?;
    show_provider_config(&cmd).await?;
    show_resources().await?;
    show_prices(&cmd).await?;
    Ok(0)
}
