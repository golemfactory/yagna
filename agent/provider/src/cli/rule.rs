use anyhow::Result;
use structopt::StructOpt;
use ya_manifest_utils::rules::{Mode, RuleStore};

use crate::startup_config::ProviderConfig;

#[derive(StructOpt, Clone, Debug)]
pub enum RuleCommand {
    Set(SetRule),
    List,
}

#[derive(StructOpt, Clone, Debug)]
pub enum SetRule {
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
        SetRule::Everyone { mode } => rules.set_everyone_mode(mode),
        SetRule::AuditedPayload { certificate, mode } => match certificate {
            Some(_) => todo!("Setting rule for specific certificate isn't implemented yet"),
            None => rules.set_default_audited_payload_mode(mode),
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
