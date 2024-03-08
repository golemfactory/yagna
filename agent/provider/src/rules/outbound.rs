use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use structopt::StructOpt;
use strum_macros::{Display, EnumString, EnumVariantNames};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OutboundConfig {
    pub enabled: bool,
    pub everyone: Mode,
    #[serde(default)]
    pub audited_payload: HashMap<String, CertRule>,
    #[serde(default)]
    pub partner: HashMap<String, CertRule>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CertRule {
    pub mode: Mode,
    pub description: String,
}

#[derive(
    StructOpt,
    Clone,
    Debug,
    Serialize,
    Deserialize,
    Eq,
    PartialEq,
    Display,
    EnumString,
    EnumVariantNames,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Mode {
    All,
    None,
    Whitelist,
}

#[derive(PartialEq, Eq, Display, Debug, Clone, Serialize, Deserialize)]
pub enum OutboundRule {
    Partner,
    AuditedPayload,
    Everyone,
}
