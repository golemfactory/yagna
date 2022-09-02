use structopt::StructOpt;

use ya_manifest_utils::{
    matching::domain::{pattern_to_id, DomainPattern, DomainPatterns},
    ArgMatch,
};
use ya_utils_cli::{CommandOutput, ResponseTable};

use crate::startup_config::ProviderConfig;

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub enum WhitelistConfig {
    /// List domain whitelist patterns
    List,
    /// Add new domain whitelist patterns
    Add(Add),
    /// Remove domain whitelist patterns
    Remove(Remove),
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub struct Add {
    /// Domain whitelist patterns
    #[structopt(long)]
    patterns: Vec<String>,

    /// Domain whitelist patterns type
    #[structopt(long)]
    pattern_type: ArgMatch,
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub struct Remove {
    /// Domain whitelist pattern ids.
    ids: Vec<String>,
}

impl WhitelistConfig {
    pub fn run(self, config: ProviderConfig) -> anyhow::Result<()> {
        match self {
            WhitelistConfig::List => list(config),
            WhitelistConfig::Add(cmd) => add(config, cmd),
            WhitelistConfig::Remove(cmd) => remove(config, cmd),
        }
    }
}

fn list(config: ProviderConfig) -> anyhow::Result<()> {
    let mut patterns = DomainPatterns::load_or_create(&config.domain_whitelist_file)?;
    let table = WhitelistTable::from(patterns);
    table.print(&config)
}

fn add(config: ProviderConfig, add: Add) -> anyhow::Result<()> {
    todo!()
}

fn remove(config: ProviderConfig, remove: Remove) -> anyhow::Result<()> {
    todo!()
}

struct WhitelistTable {
    table: ResponseTable,
}

impl WhitelistTable {
    pub fn new() -> Self {
        let columns = vec!["ID".to_string(), "Pattern".to_string(), "Type".to_string()];
        let values = vec![];
        let table = ResponseTable { columns, values };
        Self { table }
    }

    fn add(&mut self, pattern: DomainPattern) {
        let id = pattern_to_id(&pattern);
        let row = serde_json::json! {[ id, pattern.domain, pattern.domain_match ]};
        self.table.values.push(row);
    }

    pub fn print(self, config: &ProviderConfig) -> anyhow::Result<()> {
        let output = CommandOutput::from(self.table);
        output.print(config.json)?;
        Ok(())
    }
}

impl From<DomainPatterns> for WhitelistTable {
    fn from(domain_patterns: DomainPatterns) -> Self {
        let mut table = WhitelistTable::new();
        for pattern in domain_patterns.patterns {
            table.add(pattern);
        }
        table
    }
}
