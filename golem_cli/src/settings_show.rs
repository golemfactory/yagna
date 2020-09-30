use crate::utils::{get_command_json_output, move_string_out_of_json};
use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct ProviderConfig {
    pub node_name: String,
    // pub subnet: Option<String>,
}

pub async fn get_provider_config() -> Result<ProviderConfig> {
    let output = get_command_json_output("ya-provider", &["config", "get", "--json"]).await?;
    Ok(serde_json::from_value::<ProviderConfig>(output)?)
}

async fn show_provider_config() -> Result<()> {
    let provider_config = get_provider_config().await?;
    println!("node name: {}", provider_config.node_name);
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
    println!("shared resources:");
    println!("\tcores:\t{}", resources.cpu_threads);
    println!("\tmemory:\t{} GiB", resources.mem_gib);
    println!("\tdisk:\t{} GiB", resources.storage_gib);
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Preset {
    // name: String,
    // exeunit_name: String,
    // pricing_model: String,
    usage_coeffs: Prices,
}

#[derive(Copy, Clone, PartialEq, Deserialize)]
struct Prices {
    duration: f64,
    cpu: f64,
    initial: f64,
}

async fn get_prices() -> Result<Prices> {
    let profiles = get_command_json_output("ya-provider", &["preset", "list", "--json"]).await?;
    let mut profiles = serde_json::from_value::<Vec<Preset>>(profiles)?;
    let prices = profiles
        .drain(..)
        .map(|preset| preset.usage_coeffs)
        .collect::<Vec<_>>();

    if prices.is_empty() {
        bail!("No preset defined");
    }

    let are_all_the_same = prices.windows(2).all(|w| w[0] == w[1]);
    if !are_all_the_same {
        bail!("Inconsistend pricing. Please use \"ya-provider preset list\" comand to see the prices. Use \"golem settings set\" to set consistent pricing.");
    }

    Ok(prices[0])
}

pub async fn show_prices() -> Result<()> {
    let prices = get_prices().await?;
    println!("Pricing:");
    println!("\tStarting fee:\t{}", prices.initial);
    println!("\tEnv per hour:\t{}", prices.duration);
    println!("\tCpu per hour:\t{}", prices.cpu);
    Ok(())
}

pub async fn run() -> Result</*exit code*/ i32> {
    Ok([
        show_provider_config().await,
        show_resources().await,
        show_prices().await,
    ]
    .iter()
    .map(|result| {
        if let Err(e) = result {
            log::error!("{}", e);
            1
        } else {
            0
        }
    })
    .max()
    .unwrap())
}
