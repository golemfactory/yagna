use crate::rules::CertRule;
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
use ya_manifest_utils::CompositeKeystore;
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
    AuditedPayload(RuleWithCert),
    Partner(RuleWithCert),
}

#[derive(StructOpt, Clone, Debug)]
pub struct CertId {
    cert_id: String,
    #[structopt(short, long, possible_values = Mode::VARIANTS)]
    mode: Mode,
}

#[derive(StructOpt, Clone, Debug)]
pub enum AuditedPayloadRuleWithCert {
    CertId(CertId),
    ImportCert {
        import_cert: PathBuf,
        #[structopt(short, long, possible_values = Mode::VARIANTS)]
        mode: Mode,
        /// When importing chain of X.509 certificates set rule to every certificate in a chain.
        /// By default rule is assigned only to last (leaf) certificate of a chain.
        #[structopt(short, long)]
        whole_chain: bool,
    },
}

#[derive(StructOpt, Clone, Debug)]
pub enum RuleWithCert {
    CertId(CertId),
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
            SetOutboundRule::AuditedPayload(RuleWithCert::CertId(CertId { cert_id, mode })) => {
                rules.set_audited_mode(cert_id, mode)
            }
            SetOutboundRule::AuditedPayload(RuleWithCert::ImportCert { import_cert, mode }) => {
                let mut keystore = CompositeKeystore::load(&rules.cert_dir)?;

                let AddResponse {
                    invalid,
                    leaf_cert_ids,
                    ..
                } = keystore.add_golem_cert(&AddParams {
                    certs: vec![import_cert],
                })?;

                for cert_path in invalid {
                    log::error!("Failed to import {cert_path:?}. Partner mode can be set only for Golem certificate.");
                }

                rules.keystore.reload(&rules.cert_dir)?;

                for cert_id in leaf_cert_ids {
                    rules.set_audited_mode(cert_id, mode.clone())?;
                }

                Ok(())
            }
            SetOutboundRule::Partner(RuleWithCert::CertId(CertId { cert_id, mode })) => {
                rules.set_partner_mode(cert_id, mode)
            }
            SetOutboundRule::Partner(RuleWithCert::ImportCert { import_cert, mode }) => {
                let mut keystore = CompositeKeystore::load(&rules.cert_dir)?;

                let AddResponse {
                    invalid,
                    leaf_cert_ids,
                    ..
                } = keystore.add_golem_cert(&AddParams {
                    certs: vec![import_cert],
                })?;

                for cert_path in invalid {
                    log::error!("Failed to import {cert_path:?}. Partner mode can be set only for Golem certificate.");
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
        for (cert_id, rule) in audited_payload.iter() {
            let row =
                serde_json::json! {[ "Audited-Payload", rule.mode, cert_id, rule.description ]};
            self.table.values.push(row);
        }
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
        table.add_audited_payload(&outbound.audited_payload);
        table.add_partner(&outbound.partner);

        table
    }
}
