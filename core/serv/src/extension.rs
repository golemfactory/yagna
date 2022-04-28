use std::collections::{BTreeMap, HashSet};
use std::convert::{TryFrom, TryInto};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use std::{env, fs};

use anyhow::{anyhow, bail, Context};
use futures::{FutureExt, StreamExt, TryFutureExt};
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use strum::{EnumString, EnumVariantNames};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use ya_client_model::NodeId;

use ya_core_model as model;
use ya_core_model::appkey::AppKey;
use ya_service_api::CommandOutput;
use ya_service_bus::typed as bus;

const APP_NAME: &'static str = structopt::clap::crate_name!();
const DIR_NAME: &'static str = "extensions";

pub async fn run_command<T: StructOpt>(
    args: Vec<String>,
    print_help: bool,
) -> anyhow::Result<CommandOutput> {
    match Extension::find(args) {
        Ok(extension) => match extension.execute(None).await? {
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

    let data_dir = data_dir.as_ref().to_path_buf();
    let mut extensions = ExtensionManager::new(&data_dir)?.read_conf().await?;
    extensions.retain(|_, conf| conf.autostart);

    if extensions.is_empty() {
        log::info!("No extensions selected for autostart");
        return Ok(());
    }

    let ctx = ExtensionCtx {
        node_id,
        app_key: app_key.clone(),
        data_dir: data_dir.clone(),
        api_url: api_url.clone(),
        gsb_url: gsb_url.clone(),
    };

    extensions.into_iter().for_each(|(name, conf)| {
        let result: Result<Extension, _> = (name.clone(), conf).try_into();
        match result {
            Ok(ext) => {
                tokio::task::spawn_local(monitor(ext, ctx.clone()));
            }
            Err(err) => {
                log::warn!("Unable to start extension '{name}': {err}");
            }
        };
    });

    Ok(())
}

async fn resolve_identity_and_key() -> anyhow::Result<(NodeId, AppKey)> {
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

    let app_key = app_key
        .get(0)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("App key for default identity was not found"))?;

    Ok((node_id, app_key))
}

async fn monitor(mut extension: Extension, ctx: ExtensionCtx) {
    let name = extension.name.clone();
    extension.output = Output::CommonLog;

    let interrupted = tokio::signal::ctrl_c();
    let restart_loop = async move {
        loop {
            log::info!("Extension `{name}` is starting");

            match extension.execute(Some(ctx.clone())).await {
                Ok(0) => {
                    log::info!("Extension '{name}' finished");
                    break;
                }
                Ok(c) => log::warn!("Extension '{name}' failed with exit code {c}"),
                Err(e) => log::warn!("Extension '{name}' failed with error: {e}"),
            }

            tokio::time::delay_for(Duration::from_secs(3)).await;
        }
    };

    futures::pin_mut!(interrupted);
    futures::pin_mut!(restart_loop);
    let _ = futures::future::select(interrupted, restart_loop).await;
}

pub struct ExtensionManager {
    path: PathBuf,
}

impl ExtensionManager {
    const FILE_NAME: &'static str = "extensions.json";

    pub fn new(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        fs::create_dir_all(&data_dir).context("Unable to create the data directory")?;
        Ok(Self {
            path: data_dir.as_ref().join(Self::FILE_NAME),
        })
    }

    pub fn list() -> BTreeMap<String, PathBuf> {
        Self::list_in(default_dirs())
    }

    pub fn list_in(dirs: Vec<PathBuf>) -> BTreeMap<String, PathBuf> {
        let prefix = format!("{}-", APP_NAME);
        let suffix = env::consts::EXE_SUFFIX;

        dirs.into_iter()
            .rev()
            .map(fs::read_dir)
            .filter_map(Result::ok)
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|n| (n.to_string(), entry.path().to_path_buf()))
            })
            .filter_map(|(name, path)| {
                if name.starts_with(&prefix) && name.ends_with(suffix) {
                    let start = prefix.len();
                    let end = name.len() - suffix.len();
                    return Some((name[start..end].to_string(), path));
                }
                None
            })
            .filter(|(_, path)| is_executable(path))
            .collect()
    }

    pub async fn update_conf<F, R>(&self, f: F) -> anyhow::Result<()>
    where
        F: for<'a> FnOnce(&'a mut BTreeMap<String, ExtensionConf>) -> R,
    {
        let mut conf = self.read_conf().await?;
        f(&mut conf);
        self.write_conf(conf).await?;
        Ok(())
    }

    pub async fn read_conf(&self) -> anyhow::Result<BTreeMap<String, ExtensionConf>> {
        let config_bytes = match tokio::fs::read(&self.path).await {
            Ok(vec) => vec,
            Err(_) => return Ok(Default::default()),
        };
        Ok(serde_json::from_slice(config_bytes.as_slice())?)
    }

    async fn write_conf(&self, conf: BTreeMap<String, ExtensionConf>) -> anyhow::Result<()> {
        let conf = serde_json::to_string(&conf)?;
        tokio::fs::write(&self.path, conf).await.context(format!(
            "Unable to write extension configuration file at {}",
            self.path.display()
        ))?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionCtx {
    node_id: NodeId,
    app_key: AppKey,
    data_dir: PathBuf,
    api_url: url::Url,
    gsb_url: Option<url::Url>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ExtensionConf {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub requires: HashSet<Requirement>,
    pub autostart: bool,
}

#[derive(
    Debug,
    Clone,
    Hash,
    Eq,
    PartialEq,
    Serialize,
    Deserialize,
    EnumString,
    EnumVariantNames,
    StructOpt,
)]
#[serde(rename_all = "kebab-case")]
#[structopt(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Requirement {
    NodeId,
    AppKey,
    DataDir,
    ApiUrl,
    GsbUrl,
}

pub struct Extension {
    name: String,
    path: PathBuf,
    args: Vec<String>,
    requires: HashSet<Requirement>,
    output: Output,
}

impl Extension {
    pub fn find(args: Vec<String>) -> anyhow::Result<Self> {
        Self::find_in(args, default_dirs())
    }

    pub fn find_in(mut args: Vec<String>, dirs: Vec<PathBuf>) -> anyhow::Result<Self> {
        if args.is_empty() {
            bail!("No command specified");
        }

        let name = args.remove(0);
        let filename = format!("{}-{}{}", APP_NAME, name, env::consts::EXE_SUFFIX);

        let path = dirs
            .iter()
            .map(|dir| dir.join(&filename))
            .find(|file| is_executable(file))
            .ok_or_else(|| anyhow!("Extension not found: {}", name))?;

        Ok(Self {
            name,
            path,
            args,
            requires: Default::default(),
            output: Output::Inherit,
        })
    }

    pub async fn execute(&self, ctx: Option<ExtensionCtx>) -> anyhow::Result<i32> {
        let mut command = Command::new(&self.path);
        command.args(&self.args);

        if let Some(ref ctx) = ctx {
            self.add_arguments(&mut command, ctx)?;
        }

        let fut = match self.output {
            Output::CommonLog => {
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
                    tokio::task::spawn_local(BufReader::new(stdout).lines().for_each(move |s| {
                        let name_ = name_.clone();
                        async move {
                            let _ = s.map(|s| log::info!("{name_}: {s}"));
                        }
                    }));

                    let stderr = child
                        .stderr
                        .take()
                        .ok_or_else(|| anyhow!("unable to capture stderr"))?;

                    tokio::task::spawn_local(BufReader::new(stderr).lines().for_each(move |s| {
                        let name_ = name.clone();
                        async move {
                            let _ = s.map(|s| log::warn!("{name_}: {s}"));
                        }
                    }));

                    child.stdin.take();
                    child.await.map_err(anyhow::Error::new)
                }
                .boxed()
            }
            _ => command.status().map_err(anyhow::Error::new).boxed(),
        };

        match fut.await {
            Ok(status) => match status.code() {
                Some(code) => Ok(code),
                None => bail!("Extension '{}' error: unknown status", self.name),
            },
            Err(err) => bail!("Extension '{}' error: {}", self.name, err),
        }
    }

    fn add_arguments(&self, command: &mut Command, ctx: &ExtensionCtx) -> anyhow::Result<()> {
        for requirement in self.requires.iter() {
            match requirement {
                Requirement::NodeId => command.arg("--node-id").arg(ctx.node_id.to_string()),
                Requirement::AppKey => command.arg("--app-key").arg(&ctx.app_key.key),
                Requirement::ApiUrl => command.arg("--api-url").arg(ctx.api_url.to_string()),
                Requirement::GsbUrl => command.arg("--gsb-url").arg(
                    ctx.gsb_url
                        .as_ref()
                        .ok_or_else(|| anyhow!("GSB URL is missing"))?
                        .to_string(),
                ),
                Requirement::DataDir => command
                    .arg("--data-dir")
                    .arg(ctx.data_dir.to_string_lossy().to_string()),
            };
        }
        Ok(())
    }
}

impl TryFrom<(String, ExtensionConf)> for Extension {
    type Error = anyhow::Error;

    fn try_from((name, conf): (String, ExtensionConf)) -> Result<Self, Self::Error> {
        let mut args = conf.args.clone();
        args.insert(0, name);

        let mut extension = Extension::find(args)?;
        extension.requires = conf.requires;

        Ok(extension)
    }
}

#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum Output {
    Inherit,
    CommonLog,
}

#[cfg(windows)]
fn default_dirs() -> Vec<PathBuf> {
    use ya_utils_path::data_dir::DataDir;

    let mut vec = vec![];
    let data_dir = DataDir::new(APP_NAME);

    if let Ok(project_dir) = data_dir.get_or_create() {
        vec.push(project_dir.join(DIR_NAME));
    }

    if let Some(env_path) = env::var_os("PATH") {
        vec.extend(env::split_paths(&env_path));
    }

    vec
}

#[cfg(unix)]
fn default_dirs() -> Vec<PathBuf> {
    let mut vec = vec![];

    if let Some(dirs) = directories::UserDirs::new() {
        vec.push(
            dirs.home_dir()
                .join(".local")
                .join("lib")
                .join(APP_NAME)
                .join(DIR_NAME),
        );
    }

    if let Some(env_path) = env::var_os("PATH") {
        vec.extend(env::split_paths(&env_path));
    }

    vec
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
