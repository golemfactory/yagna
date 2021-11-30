use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use directories::UserDirs;
use futures::channel::oneshot;
use notify::*;
use serde::{Deserialize, Serialize};
use structopt::{clap, StructOpt};
use strum::VariantNames;
use ya_client::{cli::ApiOpts, model::node_id::NodeId};

use ya_core_model::payment::local::NetworkName;
use ya_utils_path::data_dir::DataDir;

use crate::cli::clean::CleanConfig;
use crate::cli::config::ConfigConfig;
use crate::cli::exe_unit::ExeUnitsConfig;
pub use crate::cli::preset::PresetsConfig;
use crate::cli::profile::ProfileConfig;
pub(crate) use crate::config::globals::GLOBALS_JSON;
use crate::execution::{ExeUnitsRegistry, TaskRunnerConfig};
use crate::market::config::MarketConfig;
use crate::payments::PaymentsConfig;
use crate::tasks::config::TaskConfig;

lazy_static::lazy_static! {
    static ref DEFAULT_DATA_DIR: String = DataDir::new(clap::crate_name!()).to_string();

    static ref DEFAULT_PLUGINS_DIR : PathBuf = default_plugins();
}
pub(crate) const PRESETS_JSON: &'static str = "presets.json";
pub(crate) const HARDWARE_JSON: &'static str = "hardware.json";

/// Common configuration for all Provider commands.
#[derive(StructOpt, Clone, Debug)]
pub struct ProviderConfig {
    /// Descriptor file (JSON) for available ExeUnits
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "EXE_UNIT_PATH",
        default_value_os = DEFAULT_PLUGINS_DIR.as_ref(),
        required = false,
        hide_env_values = true,
    )]
    pub exe_unit_path: PathBuf,
    /// Agent data directory
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "DATA_DIR",
        default_value = &*DEFAULT_DATA_DIR,
    )]
    pub data_dir: DataDir,
    /// Provider log directory
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "PROVIDER_LOG_DIR",
    )]
    pub log_dir: Option<DataDir>,

    #[structopt(skip = GLOBALS_JSON)]
    pub globals_file: PathBuf,
    #[structopt(skip = PRESETS_JSON)]
    pub presets_file: PathBuf,
    #[structopt(skip = HARDWARE_JSON)]
    pub hardware_file: PathBuf,
    /// Max number of available CPU cores
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "YA_RT_CORES")
    ]
    pub rt_cores: Option<usize>,
    /// Max amount of available RAM (GiB)
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "YA_RT_MEM")
    ]
    pub rt_mem: Option<f64>,
    /// Max amount of available storage (GiB)
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "YA_RT_STORAGE")
    ]
    pub rt_storage: Option<f64>,

    #[structopt(long, set = clap::ArgSettings::Global)]
    pub json: bool,
}

impl ProviderConfig {
    pub fn registry(&self) -> anyhow::Result<ExeUnitsRegistry> {
        let mut r = ExeUnitsRegistry::new();
        r.register_from_file_pattern(&self.exe_unit_path)?;
        Ok(r)
    }
}

#[derive(StructOpt, Clone, Debug, Serialize, Deserialize, derive_more::Display)]
#[display(
    fmt = "{}Networks: {:?}",
    "account.map(|a| format!(\"Address: {}\n\", a)).unwrap_or(\"\".into())",
    networks
)]
pub struct ReceiverAccount {
    /// Account for payments.
    #[structopt(long, env = "YA_ACCOUNT")]
    pub account: Option<NodeId>,
    /// Payment network.
    #[structopt(long = "payment-network", env = "YA_PAYMENT_NETWORK", possible_values = NetworkName::VARIANTS, default_value = NetworkName::Mainnet.into())]
    pub networks: Vec<NetworkName>,
}

#[derive(StructOpt, Clone, Debug)]
pub struct NodeConfig {
    /// Your human readable identity in the network.
    #[structopt(long, env = "NODE_NAME", hide_env_values = true)]
    pub node_name: Option<String>,
    /// Subnetwork identifier. You can set this value to filter nodes
    /// with other identifiers than selected. Useful for test purposes.
    #[structopt(long, env = "SUBNET")]
    pub subnet: Option<String>,

    #[structopt(flatten)]
    pub account: ReceiverAccount,
}

#[derive(StructOpt, Clone)]
pub struct RunConfig {
    #[structopt(flatten)]
    pub api: ApiOpts,
    #[structopt(flatten)]
    pub node: NodeConfig,
    #[structopt(flatten)]
    pub runner: TaskRunnerConfig,
    #[structopt(flatten)]
    pub market: MarketConfig,
    #[structopt(flatten)]
    pub payment: PaymentsConfig,
    #[structopt(flatten)]
    pub tasks: TaskConfig,
    ///changes log level from info to debug
    #[structopt(long)]
    pub debug: bool,
}

#[derive(StructOpt, Clone, Debug)]
pub struct PresetNoInteractive {
    #[structopt(long)]
    pub preset_name: Option<String>,
    #[structopt(long)]
    pub exe_unit: Option<String>,
    #[structopt(long)]
    pub pricing: Option<String>,
    #[structopt(long, parse(try_from_str = parse_key_val))]
    pub price: Vec<(String, f64)>,
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(group = clap::ArgGroup::with_name("update_names").multiple(true).required(true))]
pub struct UpdateNames {
    #[structopt(long, group = "update_names")]
    pub all: bool,

    #[structopt(group = "update_names")]
    pub names: Vec<String>,
}

#[derive(StructOpt, Clone)]
#[structopt(rename_all = "kebab-case")]
#[structopt(about = clap::crate_description!())]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(version = ya_compile_time_utils::version_describe!())]
pub struct StartupConfig {
    #[structopt(flatten)]
    pub config: ProviderConfig,
    #[structopt(flatten)]
    pub commands: Commands,
}

#[derive(StructOpt, Clone)]
pub enum Commands {
    /// Run provider agent
    Run(RunConfig),
    /// Configure provider agent
    Config(ConfigConfig),
    /// Manage offer presets
    Preset(PresetsConfig),
    /// Manage hardware profiles
    Profile(ProfileConfig),
    /// Manage ExeUnits
    ExeUnit(ExeUnitsConfig),
    /// Clean up disk space
    Clean(CleanConfig),
}

#[derive(Debug)]
pub struct FileMonitor {
    pub(crate) path: PathBuf,
    pub(crate) thread_ctl: Option<oneshot::Sender<()>>,
}

impl FileMonitor {
    pub fn spawn<P, H>(path: P, handler: H) -> std::result::Result<Self, notify::Error>
    where
        P: AsRef<Path>,
        H: Fn(DebouncedEvent) -> () + Send + 'static,
    {
        let path = path.as_ref().to_path_buf();
        let path_th = path.clone();
        let (tx, rx) = mpsc::channel();
        let (tx_ctl, mut rx_ctl) = oneshot::channel();

        let watch_delay = Duration::from_secs(3);
        let sleep_delay = Duration::from_secs(2);
        let mut watcher: RecommendedWatcher = Watcher::new(tx, watch_delay)?;

        std::thread::spawn(move || {
            let mut active = false;
            loop {
                if !active {
                    match watcher.watch(&path_th, RecursiveMode::NonRecursive) {
                        Ok(_) => active = true,
                        Err(e) => log::error!("Unable to monitor path '{:?}': {}", path_th, e),
                    }
                }
                if let Ok(event) = rx.try_recv() {
                    match &event {
                        DebouncedEvent::Rename(_, _) | DebouncedEvent::Remove(_) => {
                            let _ = watcher.unwatch(&path_th);
                            active = false
                        }
                        _ => (),
                    }
                    handler(event);
                    continue;
                }

                if let Ok(Some(_)) = rx_ctl.try_recv() {
                    break;
                }
                std::thread::sleep(sleep_delay);
            }
            log::error!("Stopping file monitor: {:?}", path_th);
        });

        Ok(Self {
            path,
            thread_ctl: Some(tx_ctl),
        })
    }

    pub fn on_modified<F>(f: F) -> impl Fn(DebouncedEvent) -> ()
    where
        F: Fn(PathBuf) -> () + Send + 'static,
    {
        move |e| match e {
            DebouncedEvent::Write(p)
            | DebouncedEvent::Chmod(p)
            | DebouncedEvent::Create(p)
            | DebouncedEvent::Remove(p)
            | DebouncedEvent::Rename(_, p) => {
                f(p);
            }
            _ => (),
        }
    }
}

impl Drop for FileMonitor {
    fn drop(&mut self) {
        if let Some(sender) = self.thread_ctl.take() {
            let _ = sender.send(());
        }
    }
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

fn default_plugins() -> PathBuf {
    if let Some(mut exe) = env::current_exe().ok() {
        exe.pop();
        exe.push("plugins");
        if exe.is_dir() {
            return exe.join("ya-*.json");
        }
    }

    UserDirs::new()
        .map(|u| u.home_dir().join(".local/lib/yagna/plugins"))
        .filter(|d| d.exists())
        .map(|p| p.join("ya-*.json"))
        .unwrap_or_else(|| "/usr/lib/yagna/plugins/ya-*.json".into())
}
