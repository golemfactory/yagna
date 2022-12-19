use std::{
    fs::OpenOptions,
    io::BufReader,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

#[derive(Clone, Debug, Default)]
pub struct RuleStore {
    pub path: PathBuf,
    config: Arc<RwLock<RulesConfig>>,
}

impl RuleStore {
    pub fn load_or_create(rules_file: &Path) -> Result<Self> {
        if rules_file.exists() {
            let file = OpenOptions::new().read(true).open(rules_file)?;

            Ok(Self {
                config: Arc::new(serde_json::from_reader(BufReader::new(file))?),
                path: rules_file.to_path_buf(),
            })
        } else {
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
        Ok(std::fs::write(
            &self.path,
            serde_json::to_string_pretty(&*self.config.read().unwrap())?,
        )?)
    }

    //TODO Rafał test it automatically (with notifier)
    pub fn reload(&self) -> Result<()> {
        let new_rule_store = Self::load_or_create(&self.path)?;

        self.replace(new_rule_store);

        Ok(())
    }

    fn replace(&self, other: Self) {
        let store = std::mem::take(&mut (*other.config.write().unwrap()));

        *self.config.write().unwrap() = store;
    }

    pub fn set_everyone_mode(&self, mode: Mode) -> Result<()> {
        self.config.write().unwrap().outbound.everyone = mode;

        self.save()
    }

    pub fn set_default_audited_payload_mode(&self, mode: Mode) -> Result<()> {
        self.config
            .write()
            .unwrap()
            .outbound
            .audited_payload
            .default
            .mode = mode;

        self.save()
    }

    pub fn get_default_outbound_settings(&self) -> OutboundSettings {
        let cfg = &self.config.read().unwrap().outbound;
        OutboundSettings {
            enabled: cfg.enabled,
            everyone: cfg.everyone.clone(),
            audited_payload: cfg.audited_payload.default.mode.clone(),
        }
    }

    pub fn print(&self) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&*self.config.read().unwrap())?
        );

        Ok(())
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
                        subject: String::new(),
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

#[derive(Clone, Debug)]
pub struct OutboundSettings {
    pub enabled: bool,
    pub everyone: Mode,
    pub audited_payload: Mode,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CertRules {
    default: CertRule,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CertRule {
    mode: Mode,
    subject: String,
}

#[derive(StructOpt, Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    All,
    None,
    Whitelist,
}
