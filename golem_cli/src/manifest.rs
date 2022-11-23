use std::fs;
use std::path::Path;
use std::str::FromStr;

use anyhow::Result;
use structopt::StructOpt;
use strum_macros::{Display, EnumString};

use crate::command::YaCommand;

#[derive(StructOpt)]
pub enum ManifestBundleCommand {
    /// Extend the configuration with the contents of the bundle
    Add(ManifestBundle),
}

#[derive(StructOpt, Debug)]
pub struct ManifestBundle {
    /// Path to the decompressed bundle
    // Golemfactory specific bundle is at https://github.com/golemfactory/ya-installer-resources/releases/latest
    path: String,
}

#[derive(Display, EnumString)]
#[strum(serialize_all = "snake_case")]
enum WhitelistType {
    Strict,
    Regex,
}

pub async fn manifest_bundle(command: ManifestBundleCommand) -> Result<i32> {
    match command {
        ManifestBundleCommand::Add(ManifestBundle { path }) => add_manifest_bundle(path).await,
    }
}

pub async fn add_manifest_bundle(path: String) -> Result<i32> {
    add_certs(&path).await?;
    add_whitelisted_domains(&path).await?;

    Ok(0)
}

async fn add_certs(path: &String) -> Result<()> {
    let cert_directory = format!("{path}/certs");
    let directory = fs::read_dir(cert_directory)?;
    let certs = directory
        // canonicalize first to resolve symlinks
        .map(|path| path.and_then(|p| p.path().canonicalize()))
        // this will ignore paths that has errors (for example broken symlinks)
        .filter(|path| path.as_ref().map(|p| p.is_file()).unwrap_or(false))
        .map(|path| path.unwrap())
        .collect::<Vec<_>>();

    let cmd = YaCommand::new()?;
    let provider = cmd.ya_provider()?;
    provider.add_certs(certs).await
}

async fn add_whitelisted_domains(path: &String) -> Result<()> {
    let whitelist_directory = format!("{path}/whitelist");
    let directory = fs::read_dir(whitelist_directory)?;
    let whitelists = directory
        // canonicalize first to resolve symlinks
        .map(|path| path.and_then(|p| p.path().canonicalize()))
        // this will ignore paths that has errors (for example broken symlinks)
        .filter(|path| path.as_ref().map(|p| p.is_file()).unwrap_or(false))
        .map(|path| path.unwrap());

    for whitelist in whitelists {
        extend_whitelist(whitelist.as_path()).await?;
    }

    Ok(())
}

async fn extend_whitelist(path: &Path) -> Result<()> {
    let file_stem = path
        .file_stem()
        .and_then(|osstr| osstr.to_str())
        .ok_or_else(|| anyhow::anyhow!("Cannot determine filename from path: {:?}", path))?;

    let whitelist_type = WhitelistType::from_str(file_stem)?;
    let file_content = fs::read_to_string(path)?;
    let entries = file_content
        .lines()
        .map(|line| line.trim())
        .collect::<Vec<_>>();

    let cmd = YaCommand::new()?;
    let provider = cmd.ya_provider()?;
    provider
        .extend_whitelist(whitelist_type.to_string(), entries)
        .await
}
