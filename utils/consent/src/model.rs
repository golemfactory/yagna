use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use structopt::StructOpt;
use strum::EnumIter;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConsentEntry {
    pub consent_type: ConsentType,
    pub allowed: bool,
}

#[derive(StructOpt, Copy, Debug, Clone, Serialize, Deserialize, PartialEq, EnumIter, Eq)]
pub enum ConsentType {
    /// Consent for internal golem monitoring
    Internal,
    /// External consent for services like stats.golem.network
    External,
}

impl PartialOrd for ConsentType {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ConsentType {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_string().cmp(&other.to_string())
    }
}

pub fn extra_info(consent_type: ConsentType) -> String {
    match consent_type {
        ConsentType::Internal => "Internal Golem Network monitoring".to_string(),
        ConsentType::External => "External services like stats.golem.network".to_string(),
    }
}

pub fn full_question(consent_type: ConsentType) -> String {
    match consent_type {
        ConsentType::Internal => {
            "Do you allow to send usage data to Internal Golem Network monitoring?".to_string()
        }
        ConsentType::External => {
            "Do you allow to send essential data to external services like stats.golem.network?"
                .to_string()
        }
    }
}

impl fmt::Display for ConsentType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl ConsentType {
    pub fn to_lowercase_str(&self) -> String {
        self.to_string().to_lowercase()
    }
}

#[derive(StructOpt, Debug)]
pub enum ConsentCommand {
    /// Show current settings
    Show,
    /// Change settings
    Allow(ConsentType),
    /// Change settings
    Deny(ConsentType),
    /// Unset setting
    Unset(ConsentType),
}
