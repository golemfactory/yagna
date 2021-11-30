use crate::command::{ProviderConfig, YaCommand};
use anyhow::Result;
use byte_unit::{Byte as Bytes, ByteUnit};
use structopt::StructOpt;

use ya_provider::ReceiverAccount;

/// Manage settings
#[derive(StructOpt, Debug)]
pub struct Settings {
    #[structopt(long)]
    node_name: Option<String>,

    /// Number of shared CPU cores
    #[structopt(long, value_name = "num")]
    cores: Option<usize>,

    /// Size of shared RAM
    #[structopt(long, value_name = "bytes (like \"1.5GiB\")")]
    memory: Option<Bytes>,

    /// Size of shared disk space
    #[structopt(long, value_name = "bytes (like \"1.5GiB\")")]
    disk: Option<Bytes>,

    /// Price for starting a task
    #[structopt(long, value_name = "GLM (float)")]
    starting_fee: Option<f64>,

    /// Price for working environment per hour
    #[structopt(long, value_name = "GLM (float)")]
    env_per_hour: Option<f64>,

    /// Price for CPU per hour
    #[structopt(long, value_name = "GLM (float)")]
    cpu_per_hour: Option<f64>,

    #[structopt(flatten)]
    pub account: ReceiverAccount,
}

pub async fn run(settings: Settings) -> Result</*exit code*/ i32> {
    log::debug!("Settings: {:?}", settings);
    let cmd = YaCommand::new()?;

    if settings.node_name.is_some() {
        cmd.ya_provider()?
            .set_config(
                &ProviderConfig {
                    node_name: settings.node_name,
                    ..ProviderConfig::default()
                },
                &settings.account.networks,
            )
            .await?;
    }

    if settings.account.account.is_some() {
        cmd.ya_provider()?
            .set_config(
                &ProviderConfig {
                    account: settings.account.account,
                    ..ProviderConfig::default()
                },
                &settings.account.networks,
            )
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
            .update_classic_presets(
                settings.starting_fee,
                settings.env_per_hour.map(|p| p / 3600.0),
                settings.cpu_per_hour.map(|p| p / 3600.0),
            )
            .await?;
    }

    Ok(0)
}
