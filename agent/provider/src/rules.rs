use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::BufReader,
    path::Path,
    sync::{Arc, RwLock},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

#[derive(Clone)]
pub struct RuleStore {
    config: Arc<RwLock<RulesConfig>>,
}

impl RuleStore {
    pub fn load_or_create(rules_file: &Path) -> Result<Self> {
        if rules_file.exists() {
            let file = OpenOptions::new().read(true).open(rules_file)?;

            Ok(Self {
                config: Arc::new(serde_json::from_reader(BufReader::new(file))?),
            })
        } else {
            let config = RulesConfig {
                outbound: OutboundRules {
                    blocked: false,
                    everyone: Mode::Whitelist,
                    rules: [(
                        RuleType::AuditedPayload,
                        [(
                            "default".into(),
                            CertRule {
                                mode: Mode::All,
                                subject: String::new(),
                            },
                        )]
                        .into(),
                    )]
                    .into(),
                },
            };

            let store = Self {
                config: Arc::new(RwLock::new(config)),
            };
            store.save(rules_file)?;

            Ok(store)
        }
    }

    pub fn save(&self, rules_file: &Path) -> Result<()> {
        Ok(std::fs::write(
            rules_file,
            serde_json::to_string_pretty(&*self.config.read().unwrap())?, //TODO Rafał unwrap
        )?)
    }

    pub fn reload(&mut self, rules_file: &Path) -> Result<()> {
        let new_rule_store = Self::load_or_create(rules_file)?;

        //TODO Rafał Check if it works properly
        *self = new_rule_store;

        Ok(())
    }

    //TODO Rafał better interface without two separate functions
    pub fn set_everyone_mode(&mut self, mode: Mode) {
        self.config.write().unwrap().outbound.everyone = mode;
    }

    pub fn set_default_cert_rule(&mut self, rule_type: RuleType, mode: Mode) {
        let mut config = self.config.write().unwrap();

        let rule = config
            .outbound
            .rules
            .entry(rule_type)
            .or_insert(HashMap::new());

        rule.insert(
            "default".into(),
            CertRule {
                mode,
                subject: String::new(),
            },
        );
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

//TODO Rafał Arc<RwLock> to be used & reloaded in providerAgent
#[derive(Debug, Serialize, Deserialize)]
pub struct RulesConfig {
    outbound: OutboundRules,
}

//TODO Rafał How file will be used in negotiator?
impl RulesConfig {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboundRules {
    blocked: bool,
    everyone: Mode,
    #[serde(flatten)]
    rules: HashMap<RuleType, HashMap<String, CertRule>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CertRule {
    mode: Mode,
    subject: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Hash, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuleType {
    AuditedPayload,
}

#[derive(StructOpt, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    All,
    None,
    /// In the future we will have { whitelist: String } here probably
    Whitelist,
}
