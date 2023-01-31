use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use structopt::StructOpt;
use strum::VariantNames;
use ya_manifest_utils::policy::CertPermissions;
use ya_manifest_utils::util::cert_to_id;
use ya_manifest_utils::{KeystoreLoadResult, KeystoreManager};
use ya_utils_cli::{CommandOutput, ResponseTable};

use crate::rules::CertRule;
use crate::{
    rules::{Mode, RulesManager},
    startup_config::ProviderConfig,
};

#[derive(StructOpt, Clone, Debug)]
pub enum RuleCommand {
    /// Set Modes for specific Rules
    Set(SetRule),
    /// List active Rules and their information
    List,
}

#[derive(StructOpt, Clone, Debug)]
pub enum SetRule {
    Outbound(SetOutboundRule),
}

#[derive(StructOpt, Clone, Debug)]
pub enum SetOutboundRule {
    Disable,
    Enable,
    Everyone {
        #[structopt(short, long, possible_values = Mode::VARIANTS)]
        mode: Mode,
    },
    AuditedPayload {
        #[structopt(long)]
        cert_id: Option<String>,
        #[structopt(short, long, possible_values = Mode::VARIANTS)]
        mode: Mode,
    },
    Partner(RuleWithCert),
}

#[derive(StructOpt, Clone, Debug)]
pub enum RuleWithCert {
    CertId {
        cert_id: String,
        #[structopt(short, long, possible_values = Mode::VARIANTS)]
        mode: Mode,
    },
    ImportCert {
        import_cert: PathBuf,
        #[structopt(short, long, possible_values = Mode::VARIANTS)]
        mode: Mode,
    },
}

impl RuleCommand {
    pub fn run(self, config: ProviderConfig) -> Result<()> {
        match self {
            RuleCommand::Set(set_rule) => set(set_rule, config),
            RuleCommand::List => list(config),
        }
    }
}

fn set(set_rule: SetRule, config: ProviderConfig) -> Result<()> {
    let rules = RulesManager::load_or_create(
        &config.rules_file,
        &config.domain_whitelist_file,
        &config.cert_dir_path()?,
    )?;

    match set_rule {
        SetRule::Outbound(outbound) => match outbound {
            SetOutboundRule::Disable => rules.set_enabled(false),
            SetOutboundRule::Enable => rules.set_enabled(true),
            SetOutboundRule::Everyone { mode } => rules.set_everyone_mode(mode),
            SetOutboundRule::AuditedPayload { cert_id, mode } => match cert_id {
                Some(_) => todo!("Setting rule for specific certificate isn't implemented yet"),
                None => rules.set_default_audited_payload_mode(mode),
            },
            SetOutboundRule::Partner(RuleWithCert::CertId { cert_id, mode }) => {
                rules.set_partner_mode(cert_id, mode)
            }
            SetOutboundRule::Partner(RuleWithCert::ImportCert { import_cert, mode }) => {
                //TODO remove keystore from keystore manager
                let keystore_manager = KeystoreManager::try_new(&rules.cert_dir)?;

                let KeystoreLoadResult { loaded, skipped } =
                    keystore_manager.load_certs(&vec![import_cert])?;

                //TODO it will be removed after backward compatibility is done
                rules.keystore.permissions_manager().set_many(
                    &loaded.iter().chain(skipped.iter()).cloned().collect(),
                    vec![CertPermissions::All],
                    true,
                );
                rules
                    .keystore
                    .permissions_manager()
                    .save(&rules.cert_dir)
                    .map_err(|e| anyhow!("Failed to save permissions file: {e}"))?;

                rules.keystore.reload(&rules.cert_dir)?;

                for cert in loaded.into_iter().chain(skipped) {
                    let cert_id = cert_to_id(&cert)?;
                    rules.set_partner_mode(cert_id, mode.clone())?;
                }

                Ok(())
            }
        },
    }
}

fn list(config: ProviderConfig) -> Result<()> {
    let rules = RulesManager::load_or_create(
        &config.rules_file,
        &config.domain_whitelist_file,
        &config.cert_dir_path()?,
    )?;
    let table = RulesTable::from(rules.clone());

    if config.json {
        rules.rulestore.print()?
    } else {
        table.print()?
    };

    Ok(())
}

struct RulesTable {
    header: Option<String>,
    table: ResponseTable,
}

impl RulesTable {
    fn new() -> Self {
        let columns = vec![
            "rule".to_string(),
            "mode".to_string(),
            "certificate".to_string(),
            "description".to_string(),
        ];
        let values = vec![];
        let table = ResponseTable { columns, values };

        Self {
            header: None,
            table,
        }
    }

    fn with_header(&mut self, outbound_status: bool) {
        let status = if outbound_status {
            "enabled"
        } else {
            "disabled"
        };
        let header = format!("\nOutbound status: {status}");

        self.header = Some(header)
    }

    fn add_everyone(&mut self, outbound_everyone: &Mode) {
        let row = serde_json::json! {[ "Everyone", outbound_everyone, "", "" ]};
        self.table.values.push(row);
    }

    fn add_audited_payload(&mut self, rule: &CertRule) {
        let row = serde_json::json! {[ "Audited payload", rule.mode, "", rule.description ]};
        self.table.values.push(row);
    }

    fn add_partner(&mut self, partner: &HashMap<String, CertRule>) {
        for (cert_id, rule) in partner.iter() {
            let row = serde_json::json! {[ "Partner", rule.mode, cert_id, rule.description ]};
            self.table.values.push(row);
        }
    }

    pub fn print(self) -> Result<()> {
        let output = CommandOutput::Table {
            columns: self.table.columns,
            values: self.table.values,
            summary: vec![],
            header: self.header,
        };

        output.print(false)?;

        Ok(())
    }
}

impl From<RulesManager> for RulesTable {
    fn from(rules: RulesManager) -> Self {
        let mut table = RulesTable::new();
        let outbound = &rules.rulestore.config.read().unwrap().outbound;

        table.with_header(outbound.enabled);
        table.add_everyone(&outbound.everyone);
        table.add_audited_payload(&outbound.audited_payload.default);
        table.add_partner(&outbound.partner);

        table
    }
}
