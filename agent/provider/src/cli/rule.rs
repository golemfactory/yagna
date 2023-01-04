use anyhow::Result;
use structopt::StructOpt;

use crate::{
    rules::{Mode, RuleStore},
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
    let rules = RuleStore::load_or_create(&config.rules_file)?;

    match set_rule {
        SetRule::Outbound(outbound) => match outbound {
            SetOutboundRule::Disable => rules.set_enabled(false),
            SetOutboundRule::Enable => rules.set_enabled(true),
            SetOutboundRule::Everyone { mode } => rules.set_everyone_mode(mode),
            SetOutboundRule::AuditedPayload { certificate, mode } => match certificate {
                Some(_) => todo!("Setting rule for specific certificate isn't implemented yet"),
                None => rules.set_default_audited_payload_mode(mode),
            },
        },
    }
}

fn list(config: ProviderConfig) -> Result<()> {
    let rules = RuleStore::load_or_create(&config.rules_file)?;

    if config.json {
        rules.print()?;
    } else {
        todo!("Printing pretty table isn't implemented yet")
    }

    Ok(())
}
