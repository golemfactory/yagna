use crate::cli::println_conditional;
use crate::rules::{CertWithRules, RulesManager};
use crate::startup_config::ProviderConfig;
use chrono::{DateTime, SecondsFormat, Utc};
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use structopt::StructOpt;
use ya_manifest_utils::keystore::{
    AddParams, AddResponse, Cert, Keystore, RemoveParams, RemoveResponse,
};
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
        added, duplicated, ..
    } = rules.keystore.add(&add.into())?;

    log_expired_certs(added.iter().chain(duplicated.iter()));

    if !added.is_empty() {
        println_conditional(&config, "Added certificates:");
        print_cert_list(&config, rules.add_rules_information_to_certs(added))?;
    }

    if !duplicated.is_empty() && !config.json {
        println!("Certificates already loaded to keystore:");
        print_cert_list(&config, rules.add_rules_information_to_certs(duplicated))?;
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
        const DIGEST_PREFIX_LENGTHS: [usize; 3] = [8, 32, 128];

        // hard-code support for the use of the entire signature, regardless of its size,
        // ensure all prefixes are no longer than the signature, and remove duplicates.
        //
        // these are, by construction, sorted smallest to largest.
        let prefix_lengths = |id_len| {
            DIGEST_PREFIX_LENGTHS
                .iter()
                .map(move |&n| std::cmp::min(n, id_len))
                .chain(std::iter::once(id_len))
                .dedup()
        };

        let mut prefix_uses = HashMap::<String, u32>::new();
        for cert in &self.entries {
            for len in prefix_lengths(cert.cert.id().len()) {
                let mut prefix = cert.cert.id();
                prefix.truncate(len);

                *prefix_uses.entry(prefix).or_default() += 1;
            }
        }

        let mut ids = Vec::new();
        for cert in &self.entries {
            for len in prefix_lengths(cert.cert.id().len()) {
                let mut prefix = cert.cert.id();
                prefix.truncate(len);

                let usages = *prefix_uses
                    .get(&prefix)
                    .expect("Internal error, unexpected prefix");

                // the longest prefix (i.e. the entire fingerprint) will be unique, so
                // this condition is guaranteed to execute during the last iteration,
                // at the latest.
                if usages == 1 {
                    ids.push(prefix);
                    break;
                }
            }
        }

        let mut values = Vec::new();
        for (id_prefix, cert) in ids.into_iter().zip(self.entries.into_iter()) {
            let not_after_formatted = date_to_str(&cert.cert.not_after());
            values
                .push(serde_json::json! { [ id_prefix, cert.cert.type_name(), not_after_formatted, cert.cert.subject(), cert.format_outbound_rules()] });
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

fn log_expired_certs<'a>(certs: impl Iterator<Item = &'a Cert>) {
    let now = Utc::now();
    for cert in certs {
        if cert.not_after() < now {
            log::warn!(
                "{} certificate expired on {},\nfingerprint: {}",
                cert.type_name(),
                date_to_str(&cert.not_after()),
                cert.id()
            );
        }
    }
}
