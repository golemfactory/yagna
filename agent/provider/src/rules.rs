use std::{fs::OpenOptions, io::BufReader, path::Path};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RulesConfig {
    outbound: OutboundRules,
}

//TODO Rafał How file will be used in negotiator?
impl RulesConfig {
    pub fn load_or_create(rules_file: &Path) -> Result<RulesConfig> {
        if rules_file.exists() {
            let file = OpenOptions::new().read(true).open(rules_file)?;

            Ok(serde_json::from_reader(BufReader::new(file))?)
        } else {
            //TODO Rafał audited_manifest rule
            let config = RulesConfig {
                outbound: OutboundRules {
                    blocked: false,
                    everyone: Mode::Whitelist,
                },
            };

            config.save(rules_file)?;

            Ok(config)
        }
    }

    pub fn save(&self, rules_file: &Path) -> Result<()> {
        Ok(std::fs::write(
            rules_file,
            serde_json::to_string_pretty(&self)?,
        )?)
    }

    pub fn set_everyone_mode(&mut self, mode: Mode) {
        self.outbound.everyone = mode;
    }

    pub fn list(&self, json: bool) -> Result<()> {
        if json {
            println!("{}", serde_json::to_string_pretty(&self)?);
        } else {
            todo!("Printing pretty table isn't implemented yet")
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboundRules {
    blocked: bool,
    /// Make more rules here
    everyone: Mode,
}

//TODO Rafał remove public fields- create helper functions
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Rule {
    pub rule_type: RuleType,
    pub mode: Mode,
    pub subject: Option<String>,
    pub cert_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum RuleType {
    AuditedPayload,
}

#[derive(StructOpt, Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Mode {
    All,
    None,
    /// In the future we will have { whitelist: String } here
    Whitelist,
}
