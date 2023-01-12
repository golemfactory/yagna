use anyhow::Result;
use structopt::StructOpt;
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
        #[structopt(subcommand)]
        mode: Mode,
    },
    AuditedPayload {
        #[structopt(long)]
        certificate: Option<String>,
        #[structopt(subcommand)]
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
    let cert_dir = config.cert_dir_path()?;
    let rules =
        RulesManager::load_or_create(&config.rules_file, &config.domain_whitelist_file, &cert_dir)?;

    match set_rule {
        SetRule::Outbound(outbound) => match outbound {
            SetOutboundRule::Disable => rules.config.set_enabled(false),
            SetOutboundRule::Enable => rules.config.set_enabled(true),
            SetOutboundRule::Everyone { mode } => rules.config.set_everyone_mode(mode),
            SetOutboundRule::AuditedPayload { certificate, mode } => match certificate {
                Some(_) => todo!("Setting rule for specific certificate isn't implemented yet"),
                None => rules.config.set_default_audited_payload_mode(mode),
            },
        },
    }
}

fn list(config: ProviderConfig) -> Result<()> {
    let cert_dir = config.cert_dir_path()?;
    let rules =
        RulesManager::load_or_create(&config.rules_file, &config.domain_whitelist_file, &cert_dir)?;
    let table = RulesTable::from(rules.clone());

    if config.json {
        rules.config.print()?
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

    fn add_everyone(&mut self, outbound_everyone: Mode) {
        let row = serde_json::json! {[ "Everyone", outbound_everyone, "", "" ]};
        self.table.values.push(row);
    }

    fn add(&mut self, rule: CertRule) {
        let row = serde_json::json! {[ "Audited payload", rule.mode, "", rule.description ]};
        self.table.values.push(row);
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
        let outbound = rules.config.config.read().unwrap().outbound.clone();

        table.with_header(outbound.enabled);
        table.add_everyone(outbound.everyone);
        table.add(outbound.audited_payload.default);

        table
    }
}
