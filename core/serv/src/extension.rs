use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::{anyhow, bail};
use structopt::StructOpt;
use tokio::process::Command;
use ya_service_api::CommandOutput;

const APP_NAME: &'static str = structopt::clap::crate_name!();

pub async fn run_command<T: StructOpt>(
    args: Vec<String>,
    help: bool,
) -> anyhow::Result<CommandOutput> {
    match Extension::find(args) {
        Ok(extension) => extension.execute().await.map(|_| CommandOutput::NoOutput),
        Err(_) => {
            if help {
                let mut clap = T::clap();
                let _ = clap.print_help();
                let _ = std::io::stdout().write_all(b"\r\n");
            }
            std::process::exit(1);
        }
    }
}

pub fn list() -> BTreeMap<String, PathBuf> {
    list_in(default_dirs())
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

pub struct Extension {
    path: PathBuf,
    args: Vec<String>,
}

impl Extension {
    pub fn find(args: Vec<String>) -> anyhow::Result<Self> {
        Self::find_in(args, default_dirs())
    }

    pub fn find_in(mut args: Vec<String>, dirs: Vec<PathBuf>) -> anyhow::Result<Self> {
        if args.is_empty() {
            bail!("No command specified");
        }

        let command = args.remove(0);
        let filename = format!("{}-{}{}", APP_NAME, command, env::consts::EXE_SUFFIX);

        let path = dirs
            .iter()
            .map(|dir| dir.join(&filename))
            .find(|file| is_executable(file))
            .ok_or_else(|| anyhow!("Command not found: {}", command))?;

        Ok(Self { path, args })
    }

    pub async fn execute(&self) -> anyhow::Result<()> {
        let mut command = Command::new(&self.path);
        command.args(&self.args);

        match command.status().await {
            Ok(status) => match status.code() {
                Some(code) => match code {
                    0 => Ok(()),
                    c => std::process::exit(c),
                },
                None => bail!("Command '{}' error: unknown status", self.path.display()),
            },
            Err(err) => bail!("Command '{}' error: {}", self.path.display(), err),
        }
    }
}

#[cfg(windows)]
fn default_dirs() -> Vec<PathBuf> {
    use ya_utils_path::data_dir::DataDir;

    let mut vec = vec![];

    // FIXME: plugin path on Windows
    let data_dir = DataDir::new(APP_NAME);
    if let Ok(project_dir) = data_dir.get_or_create() {
        vec.push(project_dir.join("plugins"));
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
                .join("plugins"),
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
