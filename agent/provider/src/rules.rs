use std::{collections::BTreeMap, fs::OpenOptions, io::BufReader, path::Path};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use ya_manifest_utils::{
    util::{self, CertBasicData, CertBasicDataVisitor},
    Keystore,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RulesConfig {
    outbound: OutboundRules,
}

//TODO Rafał How file will be used in negotiator?
impl RulesConfig {
    fn load_or_create(rules_file: &Path, cert_dir: &Path) -> Result<RulesConfig> {
        if rules_file.exists() {
            let file = OpenOptions::new().read(true).open(rules_file)?;

            Ok(serde_json::from_reader(BufReader::new(file))?)
        } else {
            //load keystore and make default rules from it
            //probably save
            let rules = OutboundRules {
                blocked: false,
                rules: vec![Rule {
                    rule_type: RuleType::Everyone,
                    mode: Mode::None,
                    subject: BTreeMap::new(),
                    cert_id: None,
                }],
            };

            let rules = util::visit_certificates(&cert_dir.to_path_buf(), rules)?;

            Ok(RulesConfig { outbound: rules })
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboundRules {
    blocked: bool,
    rules: Vec<Rule>,
}

impl CertBasicDataVisitor for OutboundRules {
    fn accept(&mut self, data: CertBasicData) {}
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    rule_type: RuleType,
    mode: Mode,
    subject: BTreeMap<String, String>,
    cert_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RuleType {
    Everyone,
    AuditedPayload,
}

#[derive(StructOpt, Clone, Debug, Serialize, Deserialize)]
pub enum Mode {
    All,
    None,
    Whitelist { whitelist: String },
}
