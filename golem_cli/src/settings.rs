use crate::{
    command::{price_per_hour_to_second, ProviderConfig, YaCommand},
    setup::ConfigAccount,
};
use anyhow::Result;
use bigdecimal::BigDecimal;
use byte_unit::{Byte as Bytes, ByteUnit};
use structopt::StructOpt;

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
    #[structopt(long, value_name = "GLM")]
    starting_fee: Option<BigDecimal>,

    /// Price for working environment per hour
    #[structopt(long, value_name = "GLM")]
    env_per_hour: Option<BigDecimal>,

    /// Price for CPU per hour
    #[structopt(long, value_name = "GLM")]
    cpu_per_hour: Option<BigDecimal>,

    #[structopt(flatten)]
    pub account: ConfigAccount,
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
                &settings.account.network,
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
                &settings.account.network,
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
                settings.env_per_hour.map(price_per_hour_to_second),
                settings.cpu_per_hour.map(price_per_hour_to_second),
            )
            .await?;
    }

    Ok(0)
}
