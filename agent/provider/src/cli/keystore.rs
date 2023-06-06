use crate::cli::println_conditional;
use crate::rules::{CertWithRules, RulesManager};
use crate::startup_config::ProviderConfig;
use chrono::{DateTime, SecondsFormat, Utc};
use std::collections::HashSet;
use std::path::PathBuf;
use std::vec;
use structopt::StructOpt;
use ya_manifest_utils::keystore::{
    AddParams, AddResponse, Cert, Keystore, RemoveParams, RemoveResponse,
};
use ya_manifest_utils::short_cert_ids::shorten_cert_ids;
use ya_utils_cli::{CommandOutput, ResponseTable};

/// Manage trusted keys
///
/// Keystore stores Golem and X.509 certificates.
/// X.509 certificates are supported in PEM or DER formats and PEM certificate chains.
/// Certificates allow to accept Demands with Computation Payload Manifests which arrive with signature and app author's public certificate.
/// Certificate gets validated against certificates loaded into the keystore.
/// Certificates are stored as files in directory, that's location can be configured using '--cert-dir' param."
#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub enum KeystoreConfig {
    /// List trusted certificates
    List,
    /// Add new trusted certificates
    Add(Add),
    /// Remove trusted certificates
    Remove(Remove),
}

#[derive(StructOpt, Clone, Debug)]
pub struct Add {
    /// Paths to certificates or certificate chains
    #[structopt(
        parse(from_os_str),
        help = "Space separated list of certificate files to be added to the Keystore."
    )]
    certs: Vec<PathBuf>,
}

impl From<Add> for AddParams {
    fn from(val: Add) -> Self {
        AddParams { certs: val.certs }
    }
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub struct Remove {
    /// Certificate ids
    #[structopt(help = "Space separated list of certificates' ids. 
To find certificate id use `keystore list` command. You may use some prefix
of the id as long as it is unique.")]
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
    let rules = RulesManager::load_or_create(
        &config.rules_file,
        &config.domain_whitelist_file,
        &config.cert_dir_path()?,
    )?;
    let certs = rules.keystore.list();
    print_cert_list(&config, rules.add_rules_information_to_certs(certs))?;
    Ok(())
}

fn add(config: ProviderConfig, add: Add) -> anyhow::Result<()> {
    let mut rules = RulesManager::load_or_create(
        &config.rules_file,
        &config.domain_whitelist_file,
        &config.cert_dir_path()?,
    )?;
    let AddResponse {
        added,
        duplicated,
        invalid,
        ..
    } = rules.keystore.add(&add.into())?;

    log_not_valid_yet_certs(added.iter().chain(duplicated.iter()));

    if !added.is_empty() {
        println_conditional(&config, "Added certificates:");
        print_cert_list(&config, rules.add_rules_information_to_certs(added))?;
    }

    if !duplicated.is_empty() && !config.json {
        println_conditional(&config, "Certificates already loaded to keystore:");
        print_cert_list(&config, rules.add_rules_information_to_certs(duplicated))?;
    }

    if !invalid.is_empty() && !config.json {
        print_invalid_cert_files_list(&config, &invalid)?;
    }
    Ok(())
}

fn remove(config: ProviderConfig, remove: Remove) -> anyhow::Result<()> {
    let mut rules = RulesManager::load_or_create(
        &config.rules_file,
        &config.domain_whitelist_file,
        &config.cert_dir_path()?,
    )?;

    let all_certs = rules.keystore.list();
    let mut ids = HashSet::new();
    for remove_prefix in &remove.ids {
        let full_ids = find_ids_by_prefix(&all_certs, remove_prefix);

        if full_ids.is_empty() {
            ids.insert(remove_prefix.clone()); //won't match anyway
        } else if full_ids.len() == 1 {
            ids.insert(full_ids[0].clone());
        } else {
            println_conditional(
                &config,
                &format!(
                    "Prefix '{remove_prefix}' isn't unique, consider using full certificate id"
                ),
            );
            if config.json {
                print_cert_list(&config, Vec::new())?;
            }

            return Ok(());
        }
    }
    let remove_params = RemoveParams { ids };

    let RemoveResponse { removed } = rules.keystore.remove(&remove_params)?;
    if removed.is_empty() {
        println_conditional(&config, "No matching certificates to remove.");
        if config.json {
            print_cert_list(&config, Vec::new())?;
        }
    } else {
        println!("Removed certificates:");
        print_cert_list(&config, rules.add_rules_information_to_certs(removed))?;
    }

    Ok(())
}

fn print_cert_list(config: &ProviderConfig, certs_data: Vec<CertWithRules>) -> anyhow::Result<()> {
    let mut table_builder = CertTableBuilder::new();
    for data in certs_data {
        table_builder.add(data);
    }

    table_builder.build().print(config)?;
    Ok(())
}

fn print_invalid_cert_files_list(
    config: &ProviderConfig,
    cert_files: &[PathBuf],
) -> anyhow::Result<()> {
    let columns = vec!["Invalid certificate files".into()];
    let values = cert_files
        .iter()
        .flat_map(|path| path.to_str())
        .map(|path| serde_json::json!([path]))
        .collect();
    let table = ResponseTable { columns, values };
    CertTable { table }.print(config)
}

fn find_ids_by_prefix(certs: &[Cert], prefix: &str) -> Vec<String> {
    certs
        .iter()
        .map(|cert| cert.id())
        .filter(|id| id.starts_with(prefix))
        .collect()
}

struct CertTableBuilder {
    entries: Vec<CertWithRules>,
}

impl CertTableBuilder {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, cert: CertWithRules) {
        self.entries.push(cert)
    }

    pub fn build(self) -> CertTable {
        let long_ids: Vec<String> = self.entries.iter().map(|e| e.cert.id()).collect();

        let short_ids = shorten_cert_ids(&long_ids);
        let mut values = vec![];
        for (entry, short_id) in self.entries.into_iter().zip(short_ids) {
            let not_after_formatted = date_to_str(&entry.cert.not_after());
            values
                .push(serde_json::json! { [ short_id, entry.cert.type_name(), not_after_formatted, entry.cert.subject(), entry.format_outbound_rules()] });
        }

        let columns = vec![
            "ID".into(),
            "Type".into(),
            "Not After".into(),
            "Subject".into(),
            "Outbound Rules".into(),
        ];

        let table = ResponseTable { columns, values };

        CertTable { table }
    }
}

struct CertTable {
    table: ResponseTable,
}

impl CertTable {
    pub fn print(self, config: &ProviderConfig) -> anyhow::Result<()> {
        let output = CommandOutput::from(self.table);
        output.print(config.json)?;
        Ok(())
    }
}

fn date_to_str(date: &DateTime<Utc>) -> String {
    date.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn log_not_valid_yet_certs<'a>(certs: impl Iterator<Item = &'a Cert>) {
    let now = Utc::now();
    for cert in certs {
        if cert.not_before() > now {
            log::warn!(
                "{} certificate will not be valid before {},\nfingerprint: {}",
                cert.type_name(),
                date_to_str(&cert.not_before()),
                cert.id()
            );
        }
    }
}
