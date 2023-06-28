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
    add_whitelisted_domains(&path).await?;
    set_audited_payload_rules(&path).await?;
    set_partner_rules(&path).await?;

    Ok(0)
}

async fn set_partner_rules(path: &String) -> Result<()> {
    let certs = get_files_from_directory(&format!("{path}/golem-certs"))?;

    for cert in certs {
        let cmd = YaCommand::new()?;
        let provider = cmd.ya_provider()?;
        provider.set_cert_rule(&cert, "partner").await?;
    }

    Ok(())
}

async fn set_audited_payload_rules(path: &String) -> Result<()> {
    let certs = get_files_from_directory(&format!("{path}/certs"))?;

    for cert in certs {
        let cmd = YaCommand::new()?;
        let provider = cmd.ya_provider()?;
        provider.set_cert_rule(&cert, "audited-payload").await?;
    }

    Ok(())
}

async fn add_whitelisted_domains(path: &String) -> Result<()> {
    let whitelists = get_files_from_directory(&format!("{path}/whitelist"))?;

    for whitelist in whitelists {
        extend_whitelist(Path::new(&whitelist)).await?;
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

fn get_files_from_directory(path: &String) -> Result<Vec<String>> {
    let directory = fs::read_dir(path)?;
    directory
        // canonicalize first to resolve symlinks
        .map(|path| path.and_then(|p| p.path().canonicalize()))
        // this will ignore paths that has errors (for example broken symlinks)
        .filter(|path| path.as_ref().map(|p| p.is_file()).unwrap_or(false))
        .map(|path| Ok(path?.to_string_lossy().to_string()))
        .collect()
}
