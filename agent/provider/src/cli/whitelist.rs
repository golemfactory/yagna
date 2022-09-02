use std::collections::HashMap;

use structopt::StructOpt;

use ya_manifest_utils::{
    matching::domain::{pattern_to_id, DomainPattern, DomainPatterns},
    ArgMatch,
};
use ya_utils_cli::{CommandOutput, ResponseTable};

use crate::cli::println_conditional;
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
    #[structopt(long, short)]
    patterns: Vec<String>,

    /// Domain whitelist patterns type
    #[structopt(long = "type", short = "t")]
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
    let domain_patterns = DomainPatterns::load_or_create(&config.domain_whitelist_file)?;
    let table = WhitelistTable::from(domain_patterns);
    table.print(&config)
}

fn add(config: ProviderConfig, add: Add) -> anyhow::Result<()> {
    let domain_patterns = DomainPatterns::load_or_create(&config.domain_whitelist_file)?;
    let mut domain_patterns = DomainPatternIds::from(domain_patterns);
    let (added, skipped) = domain_patterns.add(add);
    let domain_patterns: DomainPatterns = domain_patterns.into();
    domain_patterns.save(&config.domain_whitelist_file)?;
    if !added.is_empty() {
        println_conditional(&config, "Added domain whitelist patterns:");
        WhitelistTable::from(DomainPatterns { patterns: added }).print(&config)?
    }
    if !skipped.is_empty() && !config.json {
        println_conditional(&config, "Skipped duplicated domain whitelist patterns:");
        WhitelistTable::from(DomainPatterns { patterns: skipped }).print(&config)?
    }
    Ok(())
}

fn remove(config: ProviderConfig, remove: Remove) -> anyhow::Result<()> {
    let domain_patterns = DomainPatterns::load_or_create(&config.domain_whitelist_file)?;
    let mut domain_patterns = DomainPatternIds::from(domain_patterns);
    let (removed, _) = domain_patterns.remove(remove.ids);
    let domain_patterns: DomainPatterns = domain_patterns.into();
    domain_patterns.save(&config.domain_whitelist_file)?;
    if removed.is_empty() {
        println_conditional(&config, "No matching domain whitelist pattern to remove.");
        if config.json {
            WhitelistTable::from(DomainPatterns {
                patterns: Vec::new(),
            })
            .print(&config)?
        }
    } else {
        let table = WhitelistTable::from(DomainPatterns { patterns: removed });
        println_conditional(&config, "Removed domain whitelist patterns:");
        table.print(&config)?;
    };
    Ok(())
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

struct DomainPatternIds {
    pattern_ids: HashMap<String, DomainPattern>,
}

impl From<DomainPatterns> for DomainPatternIds {
    fn from(patterns: DomainPatterns) -> Self {
        let mut pattern_ids = HashMap::new();
        let patterns = patterns.patterns;
        for pattern in patterns {
            let id = pattern_to_id(&pattern);
            pattern_ids.insert(id, pattern);
        }
        Self { pattern_ids }
    }
}

impl Into<DomainPatterns> for DomainPatternIds {
    fn into(self) -> DomainPatterns {
        let patterns = self.pattern_ids.into_values().collect();
        DomainPatterns { patterns }
    }
}

impl DomainPatternIds {
    fn remove(&mut self, ids: Vec<String>) -> (Vec<DomainPattern>, Vec<String>) {
        let mut removed = Vec::new();
        let mut skipped = Vec::new();
        for id in ids {
            if let Some(pattern) = self.pattern_ids.remove(&id) {
                removed.push(pattern);
            } else {
                skipped.push(id);
            }
        }
        (removed, skipped)
    }

    fn add(&mut self, add: Add) -> (Vec<DomainPattern>, Vec<DomainPattern>) {
        let mut added = Vec::new();
        let mut skipped = Vec::new();
        let domain_match = add.pattern_type;
        for domain in add.patterns.into_iter() {
            let domain = domain.to_lowercase();
            let pattern = DomainPattern {
                domain,
                domain_match,
            };
            let id = pattern_to_id(&pattern);
            if let Some(duplicate) = self.pattern_ids.insert(id, pattern.clone()) {
                skipped.push(duplicate);
            } else {
                added.push(pattern)
            }
        }
        (added, skipped)
    }
}
