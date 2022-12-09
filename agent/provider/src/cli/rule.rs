use anyhow::Result;
use structopt::StructOpt;

use crate::startup_config::ProviderConfig;

#[derive(StructOpt, Clone, Debug)]
pub enum OutboundConfig {
    Set(RuleConfig),
    List,
}

impl OutboundConfig {
    pub fn run(self, config: ProviderConfig) -> Result<()> {
        dbg!(&self);

        Ok(())
    }
}

#[derive(StructOpt, Clone, Debug)]
pub enum RuleConfig {
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
