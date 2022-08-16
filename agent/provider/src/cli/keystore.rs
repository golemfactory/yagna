use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::Value;
use structopt::StructOpt;

use ya_manifest_utils::util::{self, CertBasicData, CertBasicDataVisitor};
use ya_manifest_utils::KeystoreLoadResult;
use ya_utils_cli::{CommandOutput, ResponseTable};

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
    let table_builder = CertTableBuilder::new();
    let table_builder = util::visit_certificates(&cert_dir, table_builder)?;
    let table = table_builder.build();
    let output = CommandOutput::from(table);
    output.print(false);
    Ok(())
}

fn add(config: ProviderConfig, add: Add) -> anyhow::Result<()> {
    let cert_dir = cert_dir_path(&config)?;
    let keystore_manager = util::KeystoreManager::try_new(&cert_dir)?;
    match keystore_manager.load_certs(&add.certs)? {
        KeystoreLoadResult::Loaded { loaded, skipped } => {
            println!("Added certificates:");
            let certs_data = util::to_cert_data(&loaded)?;
            print_cert_list(certs_data);
            if !skipped.is_empty() {
                println!("Certificates already loaded to keystore:");
                let certs_data = util::to_cert_data(&skipped)?;
                print_cert_list(certs_data);
            }
        }
        KeystoreLoadResult::NothingNewToLoad { skipped } => {
            println!("No new certificate to add. Skipped:");
            let certs_data = util::to_cert_data(&skipped)?;
            print_cert_list(certs_data);
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
            let certs_data = util::to_cert_data(&removed)?;
            print_cert_list(certs_data);
        }
    };
    Ok(())
}

fn cert_dir_path(config: &ProviderConfig) -> anyhow::Result<PathBuf> {
    Ok(config.cert_dir.get_or_create()?)
}

fn print_cert_list(certs_data: Vec<util::CertBasicData>) {
    let mut table_builder = CertTableBuilder::new();
    for data in certs_data {
        table_builder.with_row(data);
    }
    let table = table_builder.build();
    let output = CommandOutput::from(table);
    output.print(false);
}

struct CertTableBuilder {
    columns: Vec<String>,
    values: Vec<Value>,
}

impl CertTableBuilder {
    pub fn new() -> Self {
        let columns = vec![
            "ID".to_string(),
            "Not After".to_string(),
            "Subject".to_string(),
        ];
        let values = vec![];
        Self { columns, values }
    }

    pub fn with_row(&mut self, data: CertBasicData) {
        let row = serde_json::json! {[ data.id, data.not_after, data.subject ]};
        self.values.push(row);
    }

    pub fn build(self) -> ResponseTable {
        let columns = self.columns;
        let values = self.values;
        ResponseTable { columns, values }
    }
}

impl CertBasicDataVisitor for CertTableBuilder {
    fn accept(&mut self, data: CertBasicData) {
        self.with_row(data)
    }
}
