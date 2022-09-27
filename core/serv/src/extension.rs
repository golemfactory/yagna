use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use std::{env, fs};

use anyhow::{anyhow, bail, Context};
use futures::{FutureExt, StreamExt, TryFutureExt};
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_stream::wrappers::LinesStream;

use ya_client_model::NodeId;
use ya_core_model as model;
use ya_core_model::appkey::AppKey;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::typed as bus;

const APP_NAME: &str = structopt::clap::crate_name!();
const DIR_NAME: &str = "extensions";

pub const VAR_YAGNA_EXTENSIONS_DIR: &str = "YAGNA_EXTENSIONS_DIR";

const VAR_YAGNA_DATA_DIR: &str = "YAGNA_DATA_DIR";
const VAR_YAGNA_NODE_ID: &str = "YAGNA_NODE_ID";
const VAR_YAGNA_APP_KEY: &str = "YAGNA_APP_KEY";
const VAR_YAGNA_API_URL: &str = "YAGNA_API_URL";
const VAR_YAGNA_GSB_URL: &str = "YAGNA_GSB_URL";
const VAR_YAGNA_JSON_OUTPUT: &str = "YAGNA_JSON_OUTPUT";

pub async fn run<T: StructOpt>(
    cli_ctx: &CliCtx,
    args: Vec<String>,
) -> anyhow::Result<CommandOutput> {
    let print_help = !cli_ctx.quiet;
    match Extension::find(args) {
        Ok(extension) => match extension.execute(cli_ctx.into()).await? {
            0 => Ok(CommandOutput::NoOutput),
            c => std::process::exit(c),
        },
        Err(_) => {
            if print_help {
                let mut clap = T::clap();
                let _ = clap.print_help();
                let _ = std::io::stdout().write_all(b"\r\n");
            }
            std::process::exit(1);
        }
    }
}

pub async fn autostart(
    data_dir: impl AsRef<Path>,
    api_url: &url::Url,
    gsb_url: &Option<url::Url>,
) -> anyhow::Result<()> {
    let (node_id, app_key) = resolve_identity_and_key().await?;
    let mut extensions = Extension::list();

    extensions.retain(|ext| ext.conf.autostart);
    if extensions.is_empty() {
        log::info!("No extensions selected for autostart");
        return Ok(());
    }

    let ctx = ExtensionCtx::Autostart {
        node_id,
        app_key,
        data_dir: data_dir.as_ref().to_path_buf(),
        api_url: api_url.clone(),
        gsb_url: gsb_url.clone(),
    };

    extensions.into_iter().for_each(|ext| {
        tokio::task::spawn_local(monitor(ext, ctx.clone()));
    });

    Ok(())
}

async fn resolve_identity_and_key() -> anyhow::Result<(NodeId, Option<AppKey>)> {
    let identities = bus::service(model::identity::BUS_ID)
        .call(model::identity::List {})
        .await?
        .context("Failed to call the identity service")?;

    let node_id = match identities.into_iter().filter(|i| i.is_default).next() {
        Some(i) => i.node_id,
        None => bail!("Default identity not found"),
    };

    let (app_key, _) = bus::service(model::appkey::BUS_ID)
        .call(model::appkey::List {
            identity: Some(node_id.to_string()),
            page: 1,
            per_page: 1,
        })
        .await?
        .context("Failed to call the app key service")?;

    let app_key = app_key.get(0).cloned();

    Ok((node_id, app_key))
}

async fn monitor(extension: Extension, ctx: ExtensionCtx) {
    let name = extension.name.clone();
    let interrupted = tokio::signal::ctrl_c();
    let restart_loop = async move {
        loop {
            log::info!("Extension `{name}` starting");

            match extension.execute(ctx.clone()).await {
                Ok(0) => {
                    log::info!("Extension '{name}' finished");
                    break;
                }
                Ok(c) => log::warn!("Extension '{name}' failed with exit code {c}"),
                Err(e) => log::warn!("Extension '{name}' failed with error: {e}"),
            }

            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    };

    futures::pin_mut!(interrupted);
    futures::pin_mut!(restart_loop);
    let _ = futures::future::select(interrupted, restart_loop).await;
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum ExtensionCtx {
    Cli {
        data_dir: PathBuf,
        gsb_url: Option<url::Url>,
        json_output: bool,
    },
    Autostart {
        node_id: NodeId,
        app_key: Option<AppKey>,
        data_dir: PathBuf,
        api_url: url::Url,
        gsb_url: Option<url::Url>,
    },
}

impl ExtensionCtx {
    pub fn is_autostart(&self) -> bool {
        match self {
            Self::Autostart { .. } => true,
            _ => false,
        }
    }

    fn set_env(&self, command: &mut Command) -> anyhow::Result<()> {
        let (data_dir, gsb_url) = match self {
            Self::Cli {
                data_dir,
                gsb_url,
                json_output,
            } => {
                command.env(VAR_YAGNA_JSON_OUTPUT, json_output.to_string());
                (data_dir, gsb_url)
            }
            Self::Autostart {
                data_dir,
                gsb_url,
                node_id,
                app_key,
                api_url,
            } => {
                command
                    .env(VAR_YAGNA_NODE_ID, node_id.to_string())
                    .env(VAR_YAGNA_API_URL, api_url.to_string());

                if let Some(app_key) = app_key {
                    command.env(VAR_YAGNA_APP_KEY, &app_key.key);
                }

                (data_dir, gsb_url)
            }
        };

        command.env(VAR_YAGNA_DATA_DIR, data_dir.to_string_lossy().to_string());

        if let Some(gsb_url) = gsb_url {
            command.env(VAR_YAGNA_GSB_URL, gsb_url.to_string());
        }

        Ok(())
    }
}

impl<'a> From<&'a CliCtx> for ExtensionCtx {
    fn from(ctx: &'a CliCtx) -> Self {
        Self::Cli {
            data_dir: ctx.data_dir.clone(),
            gsb_url: ctx.gsb_url.clone(),
            json_output: ctx.json_output,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExtensionConf {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub autostart: bool,
}

impl ExtensionConf {
    fn read<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let file = OpenOptions::new().read(true).open(path)?;
        let reader = std::io::BufReader::new(file);
        Ok(serde_json::from_reader(reader)?)
    }

    async fn write<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let path = path.as_ref();
        let conf = serde_json::to_string(self)?;

        tokio::fs::write(&path, conf).await.context(format!(
            "Unable to write extension configuration file at {}",
            path.display()
        ))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extension {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub path: PathBuf,
    #[serde(flatten)]
    pub conf: ExtensionConf,
}

impl PartialEq for Extension {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}

impl Eq for Extension {}

impl Extension {
    pub fn find(args: Vec<String>) -> anyhow::Result<Self> {
        Self::find_in(args, default_dirs(), 1)
    }

    pub fn find_in(args: Vec<String>, dirs: Vec<PathBuf>, depth: usize) -> anyhow::Result<Self> {
        if args.is_empty() {
            bail!("Missing extension name");
        }

        let name = &args[0];
        let filename = format!("{}-{}{}", APP_NAME, name, env::consts::EXE_SUFFIX);

        dirs.iter()
            .map(|dir| dir.join(&filename))
            .filter_map(|path| {
                if is_executable(&path) {
                    let mut args = args.clone();
                    let name = args.remove(0);
                    let conf = ExtensionConf {
                        args,
                        ..Default::default()
                    };
                    Some(Self { name, path, conf })
                } else if depth > 0 {
                    Self::find_in(args.clone(), vec![path], depth - 1).ok()
                } else {
                    None
                }
            })
            .next()
            .ok_or_else(|| anyhow!("Extension not found: {}", name))
    }

    pub fn list() -> Vec<Self> {
        Self::list_in(default_dirs(), 1)
    }

    pub fn list_in(dirs: Vec<PathBuf>, depth: usize) -> Vec<Self> {
        let prefix = format!("{}-", APP_NAME);
        let suffix = env::consts::EXE_SUFFIX;

        dirs.into_iter()
            .map(fs::read_dir)
            .filter_map(Result::ok)
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| (name.to_string(), entry.path()))
            })
            .filter_map(|(name, path)| {
                if name.starts_with(&prefix) && name.ends_with(suffix) {
                    let start = prefix.len();
                    let end = name.len() - suffix.len();
                    return Some((name[start..end].to_string(), path));
                }
                None
            })
            .fold(Default::default(), |mut coll, (name, path)| {
                if is_executable(&path) {
                    let conf = Self::conf_path(&path)
                        .and_then(ExtensionConf::read)
                        .unwrap_or_default();
                    let ext = Self { name, path, conf };
                    if !coll.contains(&ext) {
                        coll.push(ext);
                    }
                } else if depth > 0 {
                    let mut inner_set = Self::list_in(vec![path], depth - 1);
                    inner_set.retain(|ext| !coll.contains(ext));
                    coll.extend(inner_set)
                }
                coll
            })
    }

    fn conf_path<P: AsRef<Path>>(path: P) -> anyhow::Result<PathBuf> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("unable to read parent directory"))?;
        let stem = path
            .file_stem()
            .ok_or_else(|| anyhow!("unable to read file name"))?;

        let mut path = parent.join(stem);
        path.set_extension("json");
        Ok(path)
    }

    pub async fn write_conf(&self) -> anyhow::Result<()> {
        let path = Self::conf_path(&self.path)?;
        self.conf.write(&path).await
    }

    pub async fn execute(&self, ctx: ExtensionCtx) -> anyhow::Result<i32> {
        let mut command = Command::new(&self.path);
        command.args(&self.conf.args);
        command.envs(self.conf.env.clone().into_iter());

        ctx.set_env(&mut command)?;

        let fut = if ctx.is_autostart() {
            command.stdout(Stdio::piped());
            command.stderr(Stdio::piped());
            command.stdin(Stdio::null());

            let name = self.name.clone();
            async move {
                let mut child = command
                    .spawn()
                    .map_err(|e| anyhow!("unable to spawn the binary: {}", e))?;

                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| anyhow!("unable to capture stdout"))?;

                let name_ = name.clone();
                tokio::task::spawn_local(
                    LinesStream::new(BufReader::new(stdout).lines()).for_each(move |s| {
                        let name_ = name_.clone();
                        async move {
                            let _ = s.map(|s| log::info!("{name_}: {s}"));
                        }
                    }),
                );

                let stderr = child
                    .stderr
                    .take()
                    .ok_or_else(|| anyhow!("unable to capture stderr"))?;

                tokio::task::spawn_local(
                    LinesStream::new(BufReader::new(stderr).lines()).for_each(move |s| {
                        let name_ = name.clone();
                        async move {
                            let _ = s.map(|s| log::info!("{name_}: {s}"));
                        }
                    }),
                );

                child.stdin.take();
                child.wait().await.map_err(anyhow::Error::new)
            }
            .boxed()
        } else {
            command.status().map_err(anyhow::Error::new).boxed()
        };

        match fut.await {
            Ok(status) => match status.code() {
                Some(code) => Ok(code),
                None => {
                    log::info!("Extension '{}' was terminated by signal", self.name);
                    Ok(0)
                }
            },
            Err(err) => bail!("Extension '{}' error: {}", self.name, err),
        }
    }
}

#[cfg(windows)]
fn default_dirs() -> Vec<PathBuf> {
    use ya_utils_path::data_dir::DataDir;

    let mut dirs = env_dirs();
    let data_dir = DataDir::new(APP_NAME);

    if let Ok(project_dir) = data_dir.get_or_create() {
        dirs.push(project_dir.join(DIR_NAME));
    }

    if let Some(env_path) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&env_path));
    }

    dirs
}

#[cfg(unix)]
fn default_dirs() -> Vec<PathBuf> {
    let mut dirs = env_dirs();

    if let Some(user_dirs) = directories::UserDirs::new() {
        dirs.push(
            user_dirs
                .home_dir()
                .join(".local")
                .join("lib")
                .join(APP_NAME)
                .join(DIR_NAME),
        );
    }

    if let Some(env_path) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&env_path));
    }

    dirs
}

fn env_dirs() -> Vec<PathBuf> {
    match env::var_os(VAR_YAGNA_EXTENSIONS_DIR) {
        Some(env_path) => env::split_paths(&env_path).collect(),
        None => Default::default(),
    }
}

#[cfg(windows)]
fn is_executable<P: AsRef<Path>>(path: P) -> bool {
    path.as_ref().is_file()
}

#[cfg(unix)]
fn is_executable<P: AsRef<Path>>(path: P) -> bool {
    use std::os::unix::prelude::*;
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
