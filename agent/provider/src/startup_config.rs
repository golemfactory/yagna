use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use directories::UserDirs;
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
use crate::cli::keystore::KeystoreConfig;
use crate::cli::pre_install::PreInstallConfig;
pub use crate::cli::preset::PresetsConfig;
use crate::cli::profile::ProfileConfig;
use crate::cli::rule::RuleCommand;
use crate::cli::whitelist::WhitelistConfig;
pub(crate) use crate::config::globals::GLOBALS_JSON;
use crate::execution::{ExeUnitsRegistry, TaskRunnerConfig};
use crate::market::config::MarketConfig;
use crate::payments::PaymentsConfig;
use crate::tasks::config::TaskConfig;

lazy_static::lazy_static! {
    pub static ref DEFAULT_DATA_DIR: String = default_data_dir();
    pub static ref DEFAULT_PLUGINS_DIR : PathBuf = default_plugins();
}
pub(crate) const DOMAIN_WHITELIST_JSON: &str = "domain_whitelist.json";
pub(crate) const RULES_JSON: &str = "rules.json";
pub(crate) const PRESETS_JSON: &str = "presets.json";
pub(crate) const HARDWARE_JSON: &str = "hardware.json";
pub(crate) const CERT_DIR: &str = "cert-dir";

const DATA_DIR_ENV: &str = "DATA_DIR";

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
        env = DATA_DIR_ENV,
        default_value = &*DEFAULT_DATA_DIR,
    )]
    pub data_dir: DataDir,
    /// Provider log directory
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "PROVIDER_LOG_DIR",
    )]
    log_dir: Option<DataDir>,
    /// Certificates directory
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "PROVIDER_CERT_DIR",
    )]
    cert_dir: Option<DataDir>,
    #[structopt(skip = DOMAIN_WHITELIST_JSON)]
    pub domain_whitelist_file: PathBuf,
    #[structopt(skip = GLOBALS_JSON)]
    pub globals_file: PathBuf,
    #[structopt(skip = PRESETS_JSON)]
    pub presets_file: PathBuf,
    #[structopt(skip = HARDWARE_JSON)]
    pub hardware_file: PathBuf,
    #[structopt(skip = RULES_JSON)]
    pub rules_file: PathBuf,
    /// Max number of available CPU cores
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "YA_RT_CORES"
    )]
    pub rt_cores: Option<usize>,
    /// Max amount of available RAM (GiB)
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "YA_RT_MEM"
    )]
    pub rt_mem: Option<f64>,
    /// Max amount of available storage (GiB)
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "YA_RT_STORAGE"
    )]
    pub rt_storage: Option<f64>,

    #[structopt(long, set = clap::ArgSettings::Global)]
    pub json: bool,
}

impl ProviderConfig {
    pub fn registry(&self) -> anyhow::Result<ExeUnitsRegistry> {
        let mut r = ExeUnitsRegistry::default();
        r.register_from_file_pattern(&self.exe_unit_path)?;
        Ok(r)
    }

    pub fn log_dir_path(&self) -> anyhow::Result<PathBuf> {
        let log_dir = if let Some(log_dir) = &self.log_dir {
            log_dir.get_or_create()?
        } else {
            self.data_dir.get_or_create()?
        };
        Ok(log_dir)
    }

    pub fn cert_dir_path(&self) -> anyhow::Result<PathBuf> {
        let cert_dir = if let Some(cert_dir) = &self.cert_dir {
            cert_dir.get_or_create()?
        } else {
            let mut cert_dir = self.data_dir.get_or_create()?;
            cert_dir.push(CERT_DIR);
            std::fs::create_dir_all(&cert_dir)?;
            cert_dir
        };
        Ok(cert_dir)
    }
}

#[derive(StructOpt, Clone, Debug, Serialize, Deserialize, derive_more::Display)]
#[display(
    fmt = "{}Networks: {:?}",
    "account.map(|a| format!(\"Address: {}\n\", a)).unwrap_or_else(|| \"\".into())",
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

    #[structopt(long, group = "update_names")]
    pub name: Vec<String>,
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

#[allow(clippy::large_enum_variant)]
#[derive(StructOpt, Clone)]
pub enum Commands {
    /// Run provider agent
    Run(RunConfig),
    /// Configure provider agent
    Config(ConfigConfig),
    /// Manage offer presets
    Preset(PresetsConfig),
    /// Run once by the installer before any other commands
    PreInstall(PreInstallConfig),
    /// Manage hardware profiles
    Profile(ProfileConfig),
    /// Manage ExeUnits
    ExeUnit(ExeUnitsConfig),
    Keystore(KeystoreConfig),
    /// Domain Whitelist allows to accept Demands with Computation Payload Manifests
    /// which declare usage of Outbound Network but arrive with no signature.
    Whitelist(WhitelistConfig),
    /// Free up disk space by removing old exe-unit files
    Clean(CleanConfig),
    /// Manage Rule config
    Rule(RuleCommand),
}

#[derive(Debug)]
pub struct FileMonitor {
    #[allow(dead_code)]
    pub(crate) path: PathBuf,
    pub(crate) thread_ctl: Option<mpsc::Sender<()>>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileMonitorConfig {
    pub watch_delay: Duration,
    pub verbose: bool,
}

impl Default for FileMonitorConfig {
    fn default() -> Self {
        Self {
            watch_delay: Duration::from_secs(3),
            verbose: true,
        }
    }
}

impl FileMonitorConfig {
    pub fn silent() -> Self {
        Self {
            verbose: false,
            ..Default::default()
        }
    }
}

enum Element<T: Sync + Send + 'static> {
    Some(T),
    Eos,
}

fn abortable_channel<T: Sync + Send + 'static>(
) -> (mpsc::Sender<T>, Receiver<Element<T>>, mpsc::Sender<()>) {
    let (tx, rx) = mpsc::channel::<T>();
    let (txe, rxe) = mpsc::channel::<Element<T>>();
    let (tx_abort, rx_abort) = mpsc::channel();

    let txe_ = txe.clone();
    std::thread::spawn(move || {
        for element in rx.iter().map(|t| Element::Some(t)) {
            if txe_.send(element).is_err() {
                break;
            }
        }
    });

    std::thread::spawn(move || {
        if rx_abort.recv().is_ok() {
            txe.send(Element::Eos).ok();
        }
    });

    (tx, rxe, tx_abort)
}

impl FileMonitor {
    pub fn spawn<P, H>(path: P, handler: H) -> std::result::Result<Self, notify::Error>
    where
        P: AsRef<Path>,
        H: Fn(DebouncedEvent) + Send + 'static,
    {
        Self::spawn_with(path, handler, Default::default())
    }

    pub fn spawn_with<P, H>(
        path: P,
        handler: H,
        config: FileMonitorConfig,
    ) -> std::result::Result<Self, notify::Error>
    where
        P: AsRef<Path>,
        H: Fn(DebouncedEvent) + Send + 'static,
    {
        let path = path.as_ref().to_path_buf();
        let path_th = path.clone();
        let (tx, rx, abort) = abortable_channel();

        let mut watcher: RecommendedWatcher = Watcher::new(tx, config.watch_delay)
            .map_err(|e| notify::Error::Generic(format!("Creating file Watcher: {e}")))?;

        std::thread::spawn(move || {
            let mut active = false;
            loop {
                if !active {
                    match watcher.watch(&path_th, RecursiveMode::Recursive) {
                        Ok(_) => active = true,
                        Err(e) => {
                            if config.verbose {
                                log::error!("Unable to monitor path '{:?}': {}", path_th, e);
                            }
                        }
                    }
                }
                if let Ok(event) = rx.recv() {
                    match event {
                        Element::Some(event) => {
                            match &event {
                                DebouncedEvent::Rename(_, _) | DebouncedEvent::Remove(_) => {
                                    let _ = watcher.unwatch(&path_th);
                                    active = false
                                }
                                _ => (),
                            };
                            handler(event);
                        }
                        Element::Eos => {
                            let _ = watcher.unwatch(&path_th);

                            if config.verbose {
                                log::info!("Stopping file monitor: {:?}", path_th);
                            }
                            break;
                        }
                    }
                    continue;
                }
            }
        });

        Ok(Self {
            path,
            thread_ctl: Some(abort),
        })
    }

    pub fn stop(&mut self) {
        if let Some(sender) = self.thread_ctl.take() {
            let _ = sender.send(());
        }
    }

    pub fn on_modified<F>(f: F) -> impl Fn(DebouncedEvent)
    where
        F: Fn(PathBuf) + Send + 'static,
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
        self.stop();
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

fn default_data_dir() -> String {
    DataDir::new(clap::crate_name!()).to_string()
}

fn default_plugins() -> PathBuf {
    if let Ok(mut exe) = env::current_exe() {
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
