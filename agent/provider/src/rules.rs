use serde::{Deserialize, Serialize};
use structopt::StructOpt;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RulesConfig {
    outbound: OutboundRules,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboundRules {
    blocked: bool,

    rules: Vec<Rule>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    rule_type: RuleType,
    mode: Mode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RuleType {
    Everyone,
    AuditedPayload,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Everyone {
    mode: OutboundRules,
}

#[derive(StructOpt, Clone, Debug, Serialize, Deserialize)]
pub enum Mode {
    All,
    None,
    Whitelist { whitelist: String },
}
