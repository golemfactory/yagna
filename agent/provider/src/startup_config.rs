use notify::*;
use std::error::Error;
use std::path::{Path, PathBuf};
use structopt::{clap, StructOpt};

use crate::execution::{ExeUnitsRegistry, TaskRunnerConfig};
use crate::hardware::{Resources, UpdateResources};
use directories::UserDirs;
use futures::channel::oneshot;

use std::sync::mpsc;
use std::time::Duration;
use ya_client::cli::ApiOpts;
use ya_utils_path::data_dir::DataDir;

lazy_static::lazy_static! {
    static ref DEFAULT_DATA_DIR: String = DataDir::new(clap::crate_name!()).to_string();

    static ref DEFAULT_PLUGINS_DIR : PathBuf = default_plugins();
}

/// Common configuration for all Provider commands.
#[derive(StructOpt, Clone)]
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
    #[structopt(skip = "globals.json")]
    pub globals_file: PathBuf,
    #[structopt(skip = "presets.json")]
    pub presets_file: PathBuf,
    #[structopt(skip = "hardware.json")]
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

#[derive(StructOpt)]
pub struct NodeConfig {
    /// Your human readable identity in the network.
    #[structopt(long, env = "NODE_NAME", hide_env_values = true)]
    pub node_name: Option<String>,
    /// Subnetwork identifier. You can set this value to filter nodes
    /// with other identifiers than selected. Useful for test purposes.
    #[structopt(long, env = "SUBNET")]
    pub subnet: Option<String>,
}

#[derive(StructOpt)]
pub struct RunConfig {
    #[structopt(flatten)]
    pub api: ApiOpts,
    #[structopt(flatten)]
    pub node: NodeConfig,
    #[structopt(flatten)]
    pub runner_config: TaskRunnerConfig,
}

#[derive(StructOpt)]
pub enum ConfigConfig {
    Get {
        /// 'node_name' or 'subnet'. If unspecified all config is printed.
        name: Option<String>,
    },
    Set(NodeConfig),
}

#[derive(StructOpt, Clone)]
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

#[derive(StructOpt)]
#[structopt(group = clap::ArgGroup::with_name("update_names").multiple(true).required(true))]
pub struct UpdateNames {
    #[structopt(long, group = "update_names")]
    pub all: bool,

    #[structopt(group = "update_names")]
    pub names: Vec<String>,
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub enum PresetsConfig {
    /// List available presets
    List,
    /// List active presets
    Active,
    /// Create a preset
    Create {
        #[structopt(long)]
        no_interactive: bool,
        #[structopt(flatten)]
        params: PresetNoInteractive,
    },
    /// Remove a preset
    Remove { name: String },
    /// Update a preset
    Update {
        #[structopt(flatten)]
        names: UpdateNames,
        #[structopt(long)]
        no_interactive: bool,
        #[structopt(flatten)]
        params: PresetNoInteractive,
    },
    /// Activate a preset
    Activate { name: String },
    /// Deactivate a preset
    Deactivate { name: String },
    /// List available metrics
    ListMetrics,
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub enum ProfileConfig {
    /// List available profiles
    List,
    /// Show the name of an active profile
    Active,
    /// Create a new profile
    Create {
        name: String,
        #[structopt(flatten)]
        resources: Resources,
    },
    /// Update a profile
    Update {
        #[structopt(flatten)]
        names: UpdateNames,
        #[structopt(flatten)]
        resources: UpdateResources,
    },
    /// Remove an existing profile
    Remove { name: String },
    /// Activate a profile
    Activate { name: String },
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
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(version = ya_compile_time_utils::version_describe!())]
pub struct StartupConfig {
    #[structopt(flatten)]
    pub config: ProviderConfig,
    #[structopt(flatten)]
    pub commands: Commands,
}

#[derive(StructOpt)]
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

        let watch_delay = Duration::from_secs(2);
        let sleep_delay = Duration::from_secs_f32(0.5);
        let mut watcher: RecommendedWatcher = Watcher::new(tx, watch_delay)?;

        std::thread::spawn(move || {
            if let Err(e) = watcher.watch(&path_th, RecursiveMode::NonRecursive) {
                log::error!("Unable to monitor path '{:?}': {}", path_th, e);
                return;
            }
            loop {
                if let Ok(event) = rx.try_recv() {
                    handler(event);
                }
                if let Ok(Some(_)) = rx_ctl.try_recv() {
                    break;
                }
                std::thread::sleep(sleep_delay);
            }
            log::debug!("Stopping file monitor: {:?}", path_th);
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
            | DebouncedEvent::Rename(_, p) => {
                f(p);
            }
            _ => (),
        }
    }
}

impl Drop for FileMonitor {
    fn drop(&mut self) {
        self.thread_ctl.take().map(|sender| {
            let _ = sender.send(());
        });
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
    UserDirs::new()
        .map(|u| u.home_dir().join(".local/lib/yagna/plugins"))
        .filter(|d| d.exists())
        .map(|p| p.join("ya-runtime-*.json"))
        .unwrap_or("/usr/lib/yagna/plugins/ya-runtime-*.json".into())
}
