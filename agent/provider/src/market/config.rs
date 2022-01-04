use std::path::PathBuf;
use structopt::StructOpt;

use crate::startup_config::DEFAULT_DATA_DIR;
use crate::startup_config::DEFAULT_PLUGINS_DIR;

lazy_static::lazy_static! {
    pub static ref DEFAULT_NEGOTIATORS_WORKDIR_DIR: PathBuf = default_negotiators_workdir();
}

/// Configuration for ProviderMarket actor.
#[derive(StructOpt, Clone, Debug)]
pub struct MarketConfig {
    #[structopt(long, env, default_value = "20.0")]
    pub agreement_events_interval: f32,
    #[structopt(long, env, default_value = "20.0")]
    pub negotiation_events_interval: f32,
    #[structopt(long, env, default_value = "10.0")]
    pub agreement_approve_timeout: f32,
    #[structopt(skip = "you-forgot-to-set-session-id")]
    pub session_id: String,
    /// Relative to Provider DataDir
    #[structopt(long, env, default_value = "negotiations")]
    pub negotiators_workdir: String,
    /// Uses ExeUnit plugins directory by default
    #[structopt(
        long,
        default_value_os = DEFAULT_PLUGINS_DIR.as_ref(),
        required = false,
    )]
    pub negotiators_plugins: PathBuf,
}

fn default_negotiators_workdir() -> PathBuf {
    PathBuf::from(&*DEFAULT_DATA_DIR).join("negotiations")
}
