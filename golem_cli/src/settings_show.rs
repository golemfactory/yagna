use crate::utils::{get_command_json_output, move_string_out_of_json};
use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;

#[derive(Deserialize)]
pub struct ProviderConfig {
    pub node_name: String,
    pub subnet: Option<String>,
}

#[derive(Deserialize)]
pub struct RuntimeInfo {
    pub name: String,
    pub description: Option<String>,
}

pub async fn get_runtimes() -> Result<Vec<RuntimeInfo>> {
    Ok(serde_json::from_value(
        get_command_json_output("ya-provider", &["exe-unit", "list", "--json"]).await?,
    )?)
}

async fn show_provider_config() -> Result<()> {
    println!("node name: {:?}", env::var("NODE_NAME").unwrap_or_default());
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
    println!("\n\nPricing:\n");
    println!("\t{:5} NGNT for start", prices.initial);
    println!("\t{:5} NGNT per hour", prices.duration * 3600.0);
    println!("\t{:5} NGNT per cpu hour", prices.cpu * 3600.0);
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
