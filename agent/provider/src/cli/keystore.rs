use std::collections::HashSet;
use std::path::PathBuf;

use structopt::StructOpt;

use ya_manifest_utils::{util, Keystore, KeystoreLoadResult};

use crate::startup_config::ProviderConfig;

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub enum KeystoreConfig {
    /// List trusted X.509 certificates
    List,
    /// Add new trusted X.509 certificates
    Add(Add),
    /// Remove trusted X.509 certificates
    Remove(Remove),
}

#[derive(StructOpt, Clone, Debug)]
pub struct Add {
    /// Paths to X.509 certificates (PEM or DER) or certificates chains
    #[structopt(parse(from_os_str))]
    certs: Vec<PathBuf>,
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub struct Remove {
    /// Certificate ids
    ids: Vec<String>,
}

impl KeystoreConfig {
    pub fn run(self, config: ProviderConfig) -> anyhow::Result<()> {
        match self {
            KeystoreConfig::List => list(config),
            KeystoreConfig::Add(cmd) => add(config, cmd),
            KeystoreConfig::Remove(cmd) => remove(config, cmd),
        }
    }
}

fn list(config: ProviderConfig) -> anyhow::Result<()> {
    let cert_dir = cert_dir_path(&config)?;
    let keystore = Keystore::load(&cert_dir)?;
    util::print_cert_list_header();
    keystore.visit_certs(util::print_cert_list_row)?;
    Ok(())
}

fn add(config: ProviderConfig, add: Add) -> anyhow::Result<()> {
    let cert_dir = cert_dir_path(&config)?;
    let keystore_manager = util::KeystoreManager::try_new(&cert_dir)?;
    match keystore_manager.load_certs(&add.certs)? {
        KeystoreLoadResult::Loaded { loaded, skipped } => {
            println!("Added certificates:");
            util::print_cert_list(&loaded)?;
            if !skipped.is_empty() {
                println!("Certificates already loaded to keystore:");
                util::print_cert_list(&skipped)?;
            }
        }
        KeystoreLoadResult::NothingNewToLoad { skipped } => {
            println!("No new certificate to add. Skipped:");
            util::print_cert_list(&skipped)?;
        }
    }
    Ok(())
}

fn remove(config: ProviderConfig, remove: Remove) -> anyhow::Result<()> {
    let cert_dir = cert_dir_path(&config)?;
    let keystore_manager = util::KeystoreManager::try_new(&cert_dir)?;
    let ids: HashSet<String> = remove.ids.into_iter().collect();
    match keystore_manager.remove_certs(&ids)? {
        util::KeystoreRemoveResult::NothingToRemove => {
            println!("No matching certificates to remove.");
        }
        util::KeystoreRemoveResult::Removed { removed } => {
            println!("Removed certificates:");
            util::print_cert_list(&removed)?;
        }
    };
    Ok(())
}

fn cert_dir_path(config: &ProviderConfig) -> anyhow::Result<PathBuf> {
    Ok(config.cert_dir.get_or_create()?)
}
