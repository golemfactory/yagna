use std::{
    fs::OpenOptions,
    io::BufReader,
    ops::Not,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use strum::Display;

#[derive(Clone, Debug, Default)]
pub struct RuleStore {
    pub path: PathBuf,
    config: Arc<RwLock<RulesConfig>>,
}

impl RuleStore {
    pub fn load_or_create(rules_file: &Path) -> Result<Self> {
        if rules_file.exists() {
            log::debug!("Loading rule from: {}", rules_file.display());
            let file = OpenOptions::new().read(true).open(rules_file)?;

            Ok(Self {
                config: Arc::new(serde_json::from_reader(BufReader::new(file))?),
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

    fn save(&self) -> Result<()> {
        log::debug!("Saving RuleStore to: {}", self.path.display());
        Ok(std::fs::write(
            &self.path,
            serde_json::to_string_pretty(&*self.config.read().unwrap())?,
        )?)
    }

    pub fn reload(&self) -> Result<()> {
        log::debug!("Reloading RuleStore from: {}", self.path.display());
        let new_rule_store = Self::load_or_create(&self.path)?;

        self.replace(new_rule_store);

        Ok(())
    }

    fn replace(&self, other: Self) {
        let store = std::mem::take(&mut (*other.config.write().unwrap()));

        *self.config.write().unwrap() = store;
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<()> {
        log::debug!("Setting outbound enabled: {enabled}");
        self.config.write().unwrap().outbound.enabled = enabled;

        self.save()
    }

    pub fn set_everyone_mode(&self, mode: Mode) -> Result<()> {
        log::debug!("Setting outbound everyone mode: {mode}");
        self.config.write().unwrap().outbound.everyone = mode;

        self.save()
    }

    pub fn set_default_audited_payload_mode(&self, mode: Mode) -> Result<()> {
        log::debug!("Setting outbound audited_payload default mode: {mode}");
        self.config
            .write()
            .unwrap()
            .outbound
            .audited_payload
            .default
            .mode = mode;

        self.save()
    }

    pub fn always_reject_outbound(&self) -> bool {
        self.config.read().unwrap().outbound.enabled.not()
    }

    pub fn always_accept_outbound(&self) -> bool {
        self.config.read().unwrap().outbound.everyone == Mode::All
    }

    pub fn print(&self) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&*self.config.read().unwrap())?
        );

        Ok(())
    }

    pub fn check_whitelist_for_everyone(&self) -> bool {
        self.config.read().unwrap().outbound.everyone == Mode::Whitelist
    }

    pub fn accept_all_audited_payload(&self) -> bool {
        self.config
            .read()
            .unwrap()
            .outbound
            .audited_payload
            .default
            .mode
            == Mode::All
    }

    pub fn check_whitelist_for_audited_payload(&self) -> bool {
        self.config
            .read()
            .unwrap()
            .outbound
            .audited_payload
            .default
            .mode
            == Mode::Whitelist
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RulesConfig {
    outbound: OutboundConfig,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            outbound: OutboundConfig {
                enabled: true,
                everyone: Mode::None,
                audited_payload: CertRules {
                    default: CertRule {
                        mode: Mode::All,
                        description: "Default setting".into(),
                    },
                },
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OutboundConfig {
    enabled: bool,
    everyone: Mode,
    audited_payload: CertRules,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CertRules {
    default: CertRule,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CertRule {
    mode: Mode,
    description: String,
}

#[derive(StructOpt, Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Display)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    All,
    None,
    Whitelist,
}
