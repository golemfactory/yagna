use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use structopt::StructOpt;

use crate::startup_config::{ProviderConfig, DEFAULT_PLUGINS_DIR};

lazy_static::lazy_static! {
    pub static ref DEFAULT_NEGOTIATORS_PLUGINS_DIR: PathBuf = default_negotiators_plugins();
}

/// Configuration for ProviderMarket actor.
#[derive(StructOpt, Clone)]
pub struct MarketConfig {
    #[structopt(long, env, default_value = "20.0")]
    pub agreement_events_interval: f32,
    #[structopt(long, env, default_value = "20.0")]
    pub negotiation_events_interval: f32,
    #[structopt(long, env, default_value = "10.0")]
    pub agreement_approve_timeout: f32,
    #[structopt(skip = "you-forgot-to-set-session-id")]
    pub session_id: String,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "20s")]
    pub process_market_events_timeout: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "10s")]
    pub negotiators_shutdown_timeout: std::time::Duration,
    /// Relative to Provider DataDir
    #[structopt(long, env, default_value = "negotiations")]
    pub negotiators_workdir: String,
    /// Uses ExeUnit plugins directory by default
    #[structopt(
        long,
        default_value_os = DEFAULT_NEGOTIATORS_PLUGINS_DIR.as_ref(),
        required = false,
    )]
    pub negotiators_plugins: PathBuf,
    #[structopt(long, env)]
    pub create_negotiators_config: bool,
}

/// Agent configuration that will be passed to negotiators.
#[derive(Clone, Serialize, Deserialize)]
pub struct AgentNegotiatorsConfig {
    pub rules_file: PathBuf,
    pub whitelist_file: PathBuf,
    pub cert_dir: PathBuf,
}

fn default_negotiators_plugins() -> PathBuf {
    PathBuf::from(&*DEFAULT_PLUGINS_DIR)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| "/.local/lib/yagna/plugins/".into())
}

impl TryFrom<ProviderConfig> for AgentNegotiatorsConfig {
    type Error = anyhow::Error;

    fn try_from(config: ProviderConfig) -> anyhow::Result<Self> {
        let cert_dir = config.cert_dir_path()?;
        Ok(AgentNegotiatorsConfig {
            rules_file: config.rules_file,
            whitelist_file: config.domain_whitelist_file,
            cert_dir,
        })
    }
}
