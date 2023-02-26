use crate::cli::println_conditional;
use crate::startup_config::ProviderConfig;
use std::path::PathBuf;
use structopt::StructOpt;
use strum::VariantNames;

use ya_manifest_utils::keystore::{
    AddParams, AddResponse, Cert, Keystore, RemoveParams, RemoveResponse,
};
use ya_manifest_utils::policy::CertPermissions;
use ya_manifest_utils::CompositeKeystore;
use ya_utils_cli::{CommandOutput, ResponseTable};

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
    /// If not specified, no permissions will be set for certificate.
    /// If certificate already existed, permissions will be cleared.
    #[structopt(
        short,
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

impl Into<AddParams> for Add {
    fn into(self) -> AddParams {
        AddParams {
            certs: self.certs,
            permissions: self.permissions,
            whole_chain: self.whole_chain,
        }
    }
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub struct Remove {
    /// Certificate ids
    #[structopt(help = "Space separated list of X.509 certificates' ids. 
To find certificate id use `keystore list` command.")]
    ids: Vec<String>,
}

impl Into<RemoveParams> for Remove {
    fn into(self) -> RemoveParams {
        RemoveParams {
            ids: self.ids.into_iter().collect(),
        }
    }
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
    let keystore = CompositeKeystore::load(&cert_dir)?;
    let certs_data = keystore.list();
    print_cert_list(&config, certs_data)?;
    Ok(())
}

fn add(config: ProviderConfig, add: Add) -> anyhow::Result<()> {
    let cert_dir = config.cert_dir_path()?;
    let mut keystore = CompositeKeystore::load(&cert_dir)?;
    let AddResponse { added, skipped } = keystore.add(&add.into())?;

    if !added.is_empty() {
        println_conditional(&config, "Added certificates:");
        print_cert_list(&config, added)?;
    }

    if !skipped.is_empty() && !config.json {
        println!("Certificates already loaded to keystore:");
        print_cert_list(&config, skipped)?;
    }
    Ok(())
}

fn remove(config: ProviderConfig, remove: Remove) -> anyhow::Result<()> {
    let cert_dir = config.cert_dir_path()?;
    let mut keystore_manager = CompositeKeystore::load(&cert_dir)?;
    // let mut permissions_manager = keystore_manager.permissions_manager();
    let RemoveResponse { removed } = keystore_manager.remove(&remove.into())?;
    if removed.is_empty() {
        println_conditional(&config, "No matching certificates to remove.");
        if config.json {
            print_cert_list(&config, Vec::new())?;
        }
    } else {
        // permissions_manager.set_many(&removed, vec![], true);
        println!("Removed certificates:");
        print_cert_list(&config, removed)?;
    }

    // permissions_manager
    //     .save(&cert_dir)
    //     .map_err(|e| anyhow!("Failed to save permissions file: {e}"))?;
    Ok(())
}

fn print_cert_list(config: &ProviderConfig, certs_data: Vec<Cert>) -> anyhow::Result<()> {
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
            "Permissions".to_string(),
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

    pub fn add(&mut self, cert: Cert) {
        let row = serde_json::json! {[ cert.id(), "", "", "" ]};
        self.table.values.push(row)
    }
}
