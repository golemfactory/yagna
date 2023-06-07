use crate::rules::{CertRule, OutboundRule};
use crate::{
    rules::{Mode, RulesManager},
    startup_config::ProviderConfig,
};
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use structopt::StructOpt;
use strum::VariantNames;
use ya_manifest_utils::keystore::{AddParams, AddResponse, Keystore};
use ya_manifest_utils::short_cert_ids::shorten_cert_ids;
use ya_utils_cli::{CommandOutput, ResponseTable};

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
    AuditedPayload(AuditedPayloadRuleWithCert),
    Partner(PartnerRuleWithCert),
}

#[derive(StructOpt, Clone, Debug)]
pub struct CertId {
    /// Certificate id
    cert_id: String,
    #[structopt(short, long, possible_values = Mode::VARIANTS)]
    mode: Mode,
}

#[derive(StructOpt, Clone, Debug)]
pub enum AuditedPayloadRuleWithCert {
    /// Set rule for X509 certificate with given id.
    CertId(CertId),
    /// Import and set rule for X509 certificate or X509 certificates chain (rule will be assigned to last certificate in a chain).
    ImportCert {
        /// Path to X509 certificate or X509 certificates chain.
        imported_cert: PathBuf,
        #[structopt(short, long, possible_values = Mode::VARIANTS)]
        mode: Mode,
    },
}

#[derive(StructOpt, Clone, Debug)]
pub enum PartnerRuleWithCert {
    /// Set rule for Golem certificate with given id.
    CertId(CertId),
    /// Import and set rule for X509 certificate or X509 certificates chain.
    ImportCert {
        /// Path to Golem certificate.
        imported_cert: PathBuf,
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
    let mut rules = RulesManager::load_or_create(
        &config.rules_file,
        &config.domain_whitelist_file,
        &config.cert_dir_path()?,
    )?;

    match set_rule {
        SetRule::Outbound(outbound) => match outbound {
            SetOutboundRule::Disable => rules.set_enabled(false),
            SetOutboundRule::Enable => rules.set_enabled(true),
            SetOutboundRule::Everyone { mode } => rules.set_everyone_mode(mode),
            SetOutboundRule::AuditedPayload(AuditedPayloadRuleWithCert::CertId(CertId {
                cert_id,
                mode,
            })) => rules.set_audited_payload_mode(cert_id, mode),
            SetOutboundRule::AuditedPayload(AuditedPayloadRuleWithCert::ImportCert {
                imported_cert: import_cert,
                mode,
            }) => {
                // TODO change it to `rules.keystore.add` when AuditedPayload will support Golem certs.
                let AddResponse {
                    invalid,
                    leaf_cert_ids,
                    ..
                } = rules.keystore.add_x509_cert(&AddParams {
                    certs: vec![import_cert],
                })?;

                for cert_path in invalid {
                    log::error!("Failed to import X509 certificates from: {cert_path:?}.");
                }

                rules.keystore.reload(&rules.cert_dir)?;

                for cert_id in leaf_cert_ids {
                    rules.set_audited_payload_mode(cert_id, mode.clone())?;
                }

                Ok(())
            }
            SetOutboundRule::Partner(PartnerRuleWithCert::CertId(CertId { cert_id, mode })) => {
                rules.set_partner_mode(cert_id, mode)
            }
            SetOutboundRule::Partner(PartnerRuleWithCert::ImportCert {
                imported_cert: import_cert,
                mode,
            }) => {
                let AddResponse {
                    invalid,
                    leaf_cert_ids,
                    ..
                } = rules.keystore.add_golem_cert(&AddParams {
                    certs: vec![import_cert],
                })?;

                for cert_path in invalid {
                    log::error!("Failed to import Golem certificates from: {cert_path:?}.");
                }

                rules.keystore.reload(&rules.cert_dir)?;

                for cert_id in leaf_cert_ids {
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

    fn add_audited_payload(&mut self, audited_payload: &HashMap<String, CertRule>) {
        let rules: Vec<_> = audited_payload.iter().collect();
        let long_ids: Vec<String> = rules.iter().map(|e| e.0.clone()).collect();
        let short_ids = shorten_cert_ids(&long_ids);

        for ((_long_id, rule), short_id) in rules.into_iter().zip(short_ids) {
            let row = serde_json::json! {[ OutboundRule::AuditedPayload, rule.mode, short_id, rule.description ]};
            self.table.values.push(row);
        }
    }

    fn add_partner(&mut self, partner: &HashMap<String, CertRule>) {
        let rules: Vec<_> = partner.iter().collect();
        let long_ids: Vec<String> = rules.iter().map(|e| e.0.clone()).collect();
        let short_ids = shorten_cert_ids(&long_ids);

        for ((_long_id, rule), short_id) in rules.into_iter().zip(short_ids) {
            let row = serde_json::json! {[ OutboundRule::Partner, rule.mode, short_id, rule.description ]};
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
        table.add_audited_payload(&outbound.audited_payload);
        table.add_partner(&outbound.partner);

        table
    }
}
