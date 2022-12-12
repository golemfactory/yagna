use anyhow::Result;
use structopt::StructOpt;

use crate::{
    rules::{Mode, RuleType, RulesConfig},
    startup_config::ProviderConfig,
};

#[derive(StructOpt, Clone, Debug)]
pub enum RuleCommand {
    Set(SetRule),
    List,
    BlockAll,
    UnblockAll,
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
            RuleCommand::BlockAll => todo!(),
            RuleCommand::UnblockAll => todo!(),
        }
    }
}

fn set(set_rule: SetRule, config: ProviderConfig) -> Result<()> {
    let mut rules = RulesConfig::load_or_create(&config.rules_file)?;

    match set_rule {
        SetRule::Everyone { mode } => match mode {
            Mode::None => todo!("Setting mode none for Everyone rule isn't implemented yet"),
            mode => rules.set_everyone_mode(mode),
        },
        SetRule::AuditedPayload { certificate, mode } => match certificate {
            Some(_) => todo!("Setting rule for AuditedPayload isn't implemented yet"),
            None => rules.set_default_cert_rule(RuleType::AuditedPayload, mode),
        },
    }

    rules.save(&config.rules_file)?;

    Ok(())
}

fn list(config: ProviderConfig) -> Result<()> {
    let rules = RulesConfig::load_or_create(&config.rules_file)?;

    rules.list(config.json)?;

    Ok(())
}
