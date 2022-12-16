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
    config: Arc<RwLock<RulesConfig>>,
    pub path: PathBuf,
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

    pub fn save(&self) -> Result<()> {
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

    //TODO Rafał Refactor it
    fn replace(&self, other: Self) {
        let store = std::mem::take(&mut (*other.config.write().unwrap()));

        *self.config.write().unwrap() = store;
    }

    //TODO Rafał better interface without two separate functions
    pub fn set_everyone_mode(&self, mode: Mode) {
        self.config.write().unwrap().outbound.everyone = mode;
    }

    pub fn get_everyone_mode(&self) -> Mode {
        self.config.read().unwrap().outbound.everyone.clone()
    }

    //TODO Rafał Better api to be used
    pub fn set_default_audited_payload_mode(&self, mode: Mode) {
        self.config
            .write()
            .unwrap()
            .outbound
            .audited_payload
            .default
            .mode = mode;
    }

    pub fn list(&self, json: bool) -> Result<()> {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&*self.config.read().unwrap())?
            );
        } else {
            todo!("Printing pretty table isn't implemented yet")
        }

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
                blocked: false,
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
    blocked: bool,
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
    subject: String,
}

#[derive(StructOpt, Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    All,
    None,
    Whitelist,
}
