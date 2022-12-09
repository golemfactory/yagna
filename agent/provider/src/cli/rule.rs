use anyhow::Result;
use structopt::StructOpt;

use crate::startup_config::ProviderConfig;

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
    Partner {
        #[structopt(long)]
        certificate: String,
        #[structopt(subcommand)]
        mode: Mode,
    },
}

#[derive(StructOpt, Clone, Debug)]
pub struct RuleWithoutCerts {
    #[structopt(subcommand)]
    mode: Mode,
}

#[derive(StructOpt, Clone, Debug)]
pub struct RuleWithCerts {
    #[structopt(long)]
    certificate: String,
    #[structopt(subcommand)]
    mode: Mode,
}

#[derive(StructOpt, Clone, Debug)]
pub enum Mode {
    All,
    None,
    Whitelist { whitelist: String },
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
    //Read rules from file or default
    //add new / edit existing
    //save rules to file

    Ok(())
}

fn list(config: ProviderConfig) -> Result<()> {
    //Read rules from file or default
    //Print table / json depending on config

    Ok(())
}
