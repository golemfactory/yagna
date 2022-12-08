use anyhow::Result;
use structopt::StructOpt;

use crate::startup_config::ProviderConfig;

#[derive(StructOpt, Clone, Debug)]
pub enum OutboundConfig {
    SetRule(RuleConfig),
}

impl OutboundConfig {
    pub fn run(self, config: ProviderConfig) -> Result<()> {
        Ok(())
    }
}

//TODO Rafał move to some globals?
#[derive(StructOpt, Clone, Debug)]
pub enum RuleConfig {
    Everyone(Mode),
    AuditedPayload(Mode),
}

#[derive(StructOpt, Clone, Debug)]
pub enum Mode {
    All,
    None,
}
