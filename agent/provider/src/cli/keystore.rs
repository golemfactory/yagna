use anyhow::anyhow;
use std::collections::HashSet;
use std::path::PathBuf;

use structopt::StructOpt;
use strum::VariantNames;

use ya_manifest_utils::policy::CertPermissions;
use ya_manifest_utils::util::{self, CertBasicData, CertBasicDataVisitor};
use ya_manifest_utils::KeystoreLoadResult;
use ya_utils_cli::{CommandOutput, ResponseTable};

use crate::cli::println_conditional;
use crate::startup_config::ProviderConfig;

/// Manage trusted keys
///
/// Keystore stores X.509 certificates.
/// They allow to accept Demands with Computation Payload Manifests which arrive with signature and app author's public certificate.
/// Certificate gets validated against certificates loaded into the keystore.
/// Certificates are stored as files in directory, that's location can be configured using '--cert-dir' param."
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
    /// Paths to X.509 certificates or certificates chains
    #[structopt(
        parse(from_os_str),
        help = "Space separated list of X.509 certificate files (PEM or DER) or PEM certificates chains to be added to the Keystore."
    )]
    certs: Vec<PathBuf>,
    /// Set certificates permissions for signing certain Golem features.
    #[structopt(
        long,
        parse(try_from_str),
        possible_values = CertPermissions::VARIANTS,
        case_insensitive = true,
    )]
    permissions: Vec<CertPermissions>,
    /// Apply permissions to all certificates in chain found in files.
    #[structopt(short, long)]
    whole_chain: bool,
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub struct Remove {
    /// Certificate ids
    #[structopt(help = "Space separated list of X.509 certificates' ids. 
To find certificate id use `keystore list` command.")]
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
    let cert_dir = config.cert_dir_path()?;
    let table = CertTable::new();
    let table = util::visit_certificates(&cert_dir, table)?;
    table.print(&config)?;
    Ok(())
}

fn add(config: ProviderConfig, add: Add) -> anyhow::Result<()> {
    let cert_dir = config.cert_dir_path()?;
    let keystore_manager = util::KeystoreManager::try_new(&cert_dir)?;
    let mut permissions_manager = keystore_manager.permissions_manager();
    match keystore_manager.load_certs(&add.certs)? {
        KeystoreLoadResult::Loaded { loaded, skipped } => {
            println_conditional(&config, "Added certificates:");
            let certs_data = util::to_cert_data(&loaded)?;
            print_cert_list(&config, certs_data)?;
            if !skipped.is_empty() && !config.json {
                println!("Certificates already loaded to keystore:");
                let certs_data = util::to_cert_data(&skipped)?;
                print_cert_list(&config, certs_data)?;
            }
            let all_certs = loaded.into_iter().chain(skipped.into_iter()).collect();
            permissions_manager.set_many(&all_certs, add.permissions, !add.whole_chain);
        }
        KeystoreLoadResult::NothingNewToLoad { skipped } => {
            let certs_data = util::to_cert_data(&skipped)?;
            if !config.json {
                println!("No new certificate to add.");
                println!("Ignored duplicated certificates:");
                print_cert_list(&config, certs_data)?;
            } else {
                // no new certificate added, so empty list for json output
                print_cert_list(&config, Vec::new())?;
            }

            permissions_manager.set_many(&skipped, add.permissions, add.whole_chain);
        }
    }

    permissions_manager
        .save(&cert_dir)
        .map_err(|e| anyhow!("Failed to save permissions file: {e}"))?;
    Ok(())
}

fn remove(config: ProviderConfig, remove: Remove) -> anyhow::Result<()> {
    let cert_dir = config.cert_dir_path()?;
    let keystore_manager = util::KeystoreManager::try_new(&cert_dir)?;
    let ids: HashSet<String> = remove.ids.into_iter().collect();
    match keystore_manager.remove_certs(&ids)? {
        util::KeystoreRemoveResult::NothingToRemove => {
            println_conditional(&config, "No matching certificates to remove.");
            if config.json {
                print_cert_list(&config, Vec::new())?;
            }
        }
        util::KeystoreRemoveResult::Removed { removed } => {
            println!("Removed certificates:");
            let certs_data = util::to_cert_data(&removed)?;
            print_cert_list(&config, certs_data)?;
        }
    };
    Ok(())
}

fn print_cert_list(
    config: &ProviderConfig,
    certs_data: Vec<util::CertBasicData>,
) -> anyhow::Result<()> {
    let mut table = CertTable::new();
    for data in certs_data {
        table.add(data);
    }
    table.print(config)?;
    Ok(())
}

struct CertTable {
    table: ResponseTable,
}

impl CertTable {
    pub fn new() -> Self {
        let columns = vec![
            "ID".to_string(),
            "Not After".to_string(),
            "Subject".to_string(),
        ];
        let values = vec![];
        let table = ResponseTable { columns, values };
        Self { table }
    }

    pub fn print(self, config: &ProviderConfig) -> anyhow::Result<()> {
        let output = CommandOutput::from(self.table);
        output.print(config.json)?;
        Ok(())
    }

    pub fn add(&mut self, data: CertBasicData) {
        let row = serde_json::json! {[ data.id, data.not_after, data.subject ]};
        self.table.values.push(row)
    }
}

impl CertBasicDataVisitor for CertTable {
    fn accept(&mut self, data: CertBasicData) {
        self.add(data)
    }
}
