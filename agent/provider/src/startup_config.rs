use humantime::Duration;
use std::error::Error;
use std::path::PathBuf;
use structopt::{clap, StructOpt};

use ya_client::cli::ApiOpts;

/// Common configuration for all Provider commands.
#[derive(StructOpt)]
pub struct ProviderConfig {
    /// Descriptor file (JSON) for available ExeUnits
    #[structopt(
        long = "exe-unit-path",
        env = "EXE_UNIT_PATH",
        hide_env_values = true,
        default_value = "/usr/lib/yagna/plugins/exeunits-descriptor.json"
    )]
    pub exe_unit_path: PathBuf,
    #[structopt(skip = "presets.json")]
    pub presets_file: PathBuf,
}

#[derive(StructOpt)]
pub struct NodeConfig {
    /// Your human readable identity in the network.
    #[structopt(long = "node-name", env = "NODE_NAME", hide_env_values = true)]
    pub node_name: String,
    /// Credit address. Can be set same as default identity.
    /// It will be removed in future release -- agreement will specify it.
    #[structopt(
        long = "credit-address",
        env = "CREDIT_ADDRESS",
        hide_env_values = true
    )]
    pub credit_address: String,
    /// Subnetwork identifier. You can set this value to filter nodes
    /// with other identifiers than selected. Useful for test purposes.
    #[structopt(long = "subnet", env = "SUBNET")]
    pub subnet: Option<String>,
}

#[derive(StructOpt)]
pub struct RunConfig {
    #[structopt(flatten)]
    pub api: ApiOpts,
    #[structopt(flatten)]
    pub node: NodeConfig,
    /// Estimated time of shutting down provider. Provider won't take any proposal,
    /// that expires after this time point.
    #[structopt(long)]
    pub shutdown: Duration,
    /// Offer presets, that will be sent to market.
    pub presets: Vec<String>,
}

#[derive(StructOpt)]
pub struct PresetNoInteractive {
    #[structopt(long)]
    pub preset_name: Option<String>,
    #[structopt(long)]
    pub exeunit: Option<String>,
    #[structopt(long)]
    pub pricing: Option<String>,
    #[structopt(long, parse(try_from_str = parse_key_val))]
    pub price: Vec<(String, f64)>,
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub enum PresetsConfig {
    List,
    Create {
        #[structopt(long = "nointeractive")]
        nointeractive: bool,
        #[structopt(flatten)]
        params: PresetNoInteractive,
    },
    Remove {
        name: String,
    },
    Update {
        name: String,
        #[structopt(long = "nointeractive")]
        nointeractive: bool,
        #[structopt(flatten)]
        params: PresetNoInteractive,
    },
    ListMetrics,
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub enum ExeUnitsConfig {
    List,
    // TODO: Install command - could download ExeUnit and add to descriptor file.
    // TODO: Update command - could update ExeUnit.
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
#[structopt(about = clap::crate_description!())]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub struct StartupConfig {
    #[structopt(flatten)]
    pub config: ProviderConfig,
    #[structopt(flatten)]
    pub commands: Commands,
}

#[derive(StructOpt)]
pub enum Commands {
    Run(RunConfig),
    Preset(PresetsConfig),
    ExeUnit(ExeUnitsConfig),
}

/// Structopt key-value example:
/// https://github.com/TeXitoi/structopt/blob/master/examples/keyvalue.rs
fn parse_key_val<T, U>(s: &str) -> std::result::Result<(T, U), Box<dyn Error>>
where
    T: std::str::FromStr,
    T::Err: Error + 'static,
    U: std::str::FromStr,
    U::Err: Error + 'static,
{
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{}`", s))?;
    Ok((s[..pos].parse()?, s[pos + 1..].parse()?))
}
