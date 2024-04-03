use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::BufReader;
use std::ops::Not;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::rules::outbound::OutboundConfig;
use crate::rules::restrict::RestrictConfig;

#[derive(Clone, Debug)]
pub struct Rulestore {
    pub path: PathBuf,
    pub config: Arc<RwLock<RulesConfig>>,
}

impl Rulestore {
    pub fn load_or_create(rules_file: &Path) -> anyhow::Result<Self> {
        if rules_file.exists() {
            log::debug!("Loading rule from: {}", rules_file.display());
            let file = OpenOptions::new().read(true).open(rules_file)?;
            let config: RulesConfig = serde_json::from_reader(BufReader::new(file))?;
            log::debug!("Loaded rules: {:#?}", config);

            Ok(Self {
                config: Arc::new(RwLock::new(config)),
                path: rules_file.to_path_buf(),
            })
        } else {
            log::debug!("Creating default Rules configuration");
            let config = Default::default();

            let store = Self {
                config: Arc::new(RwLock::new(config)),
                path: rules_file.to_path_buf(),
            };
            store.save()?;

            Ok(store)
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        log::debug!("Saving RuleStore to: {}", self.path.display());
        Ok(std::fs::write(
            &self.path,
            serde_json::to_string_pretty(&*self.config.read().unwrap())?,
        )?)
    }

    pub fn reload(&self) -> anyhow::Result<()> {
        log::debug!("Reloading RuleStore from: {}", self.path.display());
        let new_rule_store = Self::load_or_create(&self.path)?;

        self.replace(new_rule_store);

        Ok(())
    }

    fn replace(&self, other: Self) {
        let store = std::mem::take(&mut (*other.config.write().unwrap()));

        *self.config.write().unwrap() = store;
    }

    pub fn print(&self) -> anyhow::Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&*self.config.read().unwrap())?
        );

        Ok(())
    }

    pub fn is_outbound_disabled(&self) -> bool {
        self.config.read().unwrap().outbound.enabled.not()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RulesConfig {
    pub outbound: OutboundConfig,
    #[serde(default)]
    pub blacklist: RestrictConfig,
    #[serde(default)]
    pub allow_only: RestrictConfig,
}
