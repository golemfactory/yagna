use notify::*;
use std::error::Error;
use std::path::{Path, PathBuf};
use structopt::{clap, StructOpt};

use crate::execution::{ExeUnitsRegistry, TaskRunnerConfig};
use crate::hardware::Resources;
use futures::channel::oneshot;
use std::sync::mpsc;
use std::time::Duration;
use ya_client::cli::ApiOpts;
use ya_utils_path::data_dir::DataDir;

/// Common configuration for all Provider commands.
#[derive(StructOpt)]
pub struct ProviderConfig {
    /// Descriptor file (JSON) for available ExeUnits
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "EXE_UNIT_PATH",
        default_value = "/usr/lib/yagna/plugins/ya-runtime-*.json",
        hide_env_values = true,
    )]
    pub exe_unit_path: PathBuf,
    /// Agent data directory
    #[structopt(
        long,
        set = clap::ArgSettings::Global,
        env = "DATA_DIR",
        default_value,
    )]
    pub data_dir: DataDir,
    // FIXME: workspace configuration
    #[structopt(skip = "presets.json")]
    pub presets_file: PathBuf,
    #[structopt(skip = "hardware.json")]
    pub hardware_file: PathBuf,
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
    pub node_name: String,
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
    /// Offer presets, that will be sent to market.
    pub presets: Vec<String>,
}

#[derive(StructOpt)]
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
#[structopt(rename_all = "kebab-case")]
pub enum PresetsConfig {
    List,
    Create {
        #[structopt(long)]
        no_interactive: bool,
        #[structopt(flatten)]
        params: PresetNoInteractive,
    },
    Remove {
        name: String,
    },
    Update {
        name: String,
        #[structopt(long)]
        no_interactive: bool,
        #[structopt(flatten)]
        params: PresetNoInteractive,
    },
    ListMetrics,
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub enum ProfileConfig {
    /// List available profiles
    List,
    /// Show profile details
    Show { name: String },
    /// Create a new profile
    Create {
        name: String,
        #[structopt(flatten)]
        resources: Resources,
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
    pub fn spawn<P, F>(path: P, handler: F) -> std::result::Result<Self, notify::Error>
    where
        P: AsRef<Path>,
        F: Fn(DebouncedEvent) -> () + Send + 'static,
    {
        let path = path.as_ref().to_path_buf();
        let path_th = path.clone();
        let (tx, rx) = mpsc::channel();
        let (tx_ctl, mut rx_ctl) = oneshot::channel();

        let watch_delay = Duration::from_secs(2);
        let sleep_delay = Duration::from_secs_f32(0.5);
        let mut watcher: RecommendedWatcher = Watcher::new(tx, watch_delay)?;

        std::thread::spawn(move || {
            watcher
                .watch(&path_th, RecursiveMode::NonRecursive)
                .unwrap();
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
