use anyhow::Result;
use structopt::StructOpt;

use crate::{
    rules::{Mode, Rule, RuleType, RulesConfig},
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
        certificate: String,
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
            Mode::None => todo!(),
            mode => rules.set(Rule {
                rule_type: RuleType::Everyone,
                mode,
                subject: None,
                cert_id: None,
            }),
        },
        SetRule::AuditedPayload {
            certificate: _,
            mode: _,
        } => todo!(),
    }

    rules.save(&config.rules_file)?;

    Ok(())
}

fn list(config: ProviderConfig) -> Result<()> {
    let rules = RulesConfig::load_or_create(&config.rules_file)?;

    rules.list(config.json)?;

    Ok(())
}
