use anyhow::{Context, Result};
use byte_unit::{Byte as Bytes, ByteUnit};
use structopt::{clap, StructOpt};
use tokio::process::Command;

/// Manage settings
#[derive(StructOpt, Debug)]
// "set" group requires at least one value
// see also https://github.com/TeXitoi/structopt/issues/110
// https://github.com/TeXitoi/structopt/issues/104
#[structopt(group = clap::ArgGroup::with_name("set").multiple(true).required(true))]
pub struct Settings {
    #[structopt(long, group = "set")]
    node_name: Option<String>,

    /// Number of shared CPU cores
    #[structopt(long, group = "set", value_name = "num")]
    cores: Option<i32>,

    /// Size of shared RAM
    #[structopt(long, group = "set", value_name = "bytes (like \"1.5GiB\")")]
    memory: Option<Bytes>,

    /// Size of shared disk space
    #[structopt(long, group = "set", value_name = "bytes (like \"1.5GiB\")")]
    disk: Option<Bytes>,

    /// Price for starting a task
    #[structopt(long, group = "set", value_name = "NGNT (float)")]
    starting_fee: Option<f64>,

    /// Price for working environment per hour
    #[structopt(long, group = "set", value_name = "NGNT (float)")]
    env_per_hour: Option<f64>,

    /// Price for CPU per hour
    #[structopt(long, group = "set", value_name = "NGNT (float)")]
    cpu_per_hour: Option<f64>,
}

pub async fn run(settings: Settings) -> Result</*exit code*/ i32> {
    log::debug!("settings: {:?}", settings);

    if let Some(node_name) = settings.node_name {
        let exit_status = Command::new("ya-provider")
            .args(&["config", "set", "--node-name", &node_name])
            .status()
            .await
            .context("Failed to spawn ya-provider")?;
        if !exit_status.success() {
            log::error!("Failed to update node name");
            return Ok(exit_status.code().unwrap_or(1));
        }
    }

    if settings.cores.is_some() || settings.memory.is_some() || settings.disk.is_some() {
        let mut cmd = Command::new("ya-provider");
        cmd.arg("profile").arg("update").arg("default");

        if let Some(cores) = settings.cores {
            cmd.arg("--cpu-threads").arg(cores.to_string());
        }
        if let Some(memory) = settings.memory {
            cmd.arg("--mem-gib").arg(
                memory
                    .get_adjusted_unit(ByteUnit::GiB)
                    .get_value()
                    .to_string(),
            );
        }
        if let Some(disk) = settings.disk {
            cmd.arg("--disk").arg(
                disk.get_adjusted_unit(ByteUnit::GiB)
                    .get_value()
                    .to_string(),
            );
        }

        let exit_status = cmd.status().await.context("Failed to spawn ya-provider")?;
        log::debug!("ya-provider profile update: {:?}", exit_status);
        if !exit_status.success() {
            log::error!("Failed to update resources settings");
            return Ok(exit_status.code().unwrap_or(1));
        }
    }

    if settings.starting_fee.is_some()
        || settings.env_per_hour.is_some()
        || settings.cpu_per_hour.is_some()
    {
        let mut cmd = Command::new("ya-provider");
        cmd.args(&["preset", "update", "--all", "--no-interactive", "--price"]);

        if let Some(starting_fee) = settings.starting_fee {
            cmd.arg(format!("Init price={}", starting_fee));
        }
        if let Some(env_per_hour) = settings.env_per_hour {
            cmd.arg(format!("Duration={}", env_per_hour / 3600.0));
        }
        if let Some(cpu_per_hour) = settings.cpu_per_hour {
            cmd.arg(format!("CPU={}", cpu_per_hour / 3600.0));
        }

        let exit_status = cmd.status().await.context("Failed to spawn ya-provider")?;
        log::debug!("ya-provider preset update: {:?}", exit_status);
        if !exit_status.success() {
            log::error!("Failed to update price settings");
            return Ok(exit_status.code().unwrap_or(1));
        }
    }

    Ok(0)
}
