use crate::command::{ProviderConfig, RecvAccount, YaCommand};
use anyhow::Result;
use byte_unit::{Byte as Bytes, ByteUnit};
use structopt::{clap, StructOpt};

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
    cores: Option<usize>,

    /// Size of shared RAM
    #[structopt(long, group = "set", value_name = "bytes (like \"1.5GiB\")")]
    memory: Option<Bytes>,

    /// Size of shared disk space
    #[structopt(long, group = "set", value_name = "bytes (like \"1.5GiB\")")]
    disk: Option<Bytes>,

    /// Price for starting a task
    #[structopt(long, group = "set", value_name = "GLM (float)")]
    starting_fee: Option<f64>,

    /// Price for working environment per hour
    #[structopt(long, group = "set", value_name = "GLM (float)")]
    env_per_hour: Option<f64>,

    /// Price for CPU per hour
    #[structopt(long, group = "set", value_name = "GLM (float)")]
    cpu_per_hour: Option<f64>,
    /// Wallet address
    #[structopt(long, group = "set")]
    address: Option<String>,
}

pub async fn run(settings: Settings) -> Result</*exit code*/ i32> {
    log::debug!("settings: {:?}", settings);
    let cmd = YaCommand::new()?;

    if let Some(node_name) = settings.node_name {
        cmd.ya_provider()?
            .set_config(&ProviderConfig {
                node_name: Some(node_name),
                ..ProviderConfig::default()
            })
            .await?;
    }

    if let Some(address) = settings.address {
        cmd.ya_provider()?
            .set_config(&ProviderConfig {
                account: Some(RecvAccount {
                    platform: None,
                    address,
                }),
                ..ProviderConfig::default()
            })
            .await?;
    }

    if settings.cores.is_some() || settings.memory.is_some() || settings.disk.is_some() {
        cmd.ya_provider()?
            .update_profile(
                "default",
                settings.cores,
                settings
                    .memory
                    .map(|memory| memory.get_adjusted_unit(ByteUnit::GiB).get_value()),
                settings
                    .disk
                    .map(|disk| disk.get_adjusted_unit(ByteUnit::GiB).get_value()),
            )
            .await?;
    }

    if settings.starting_fee.is_some()
        || settings.env_per_hour.is_some()
        || settings.cpu_per_hour.is_some()
    {
        cmd.ya_provider()?
            .update_all_presets(
                settings.starting_fee,
                settings.env_per_hour.map(|p| p / 3600.0),
                settings.cpu_per_hour.map(|p| p / 3600.0),
            )
            .await?;
    }

    Ok(0)
}
