use crate::api::{have_consent, to_json};
use crate::set_consent;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cmp::Ordering;
use std::fmt;
use structopt::StructOpt;
use strum::{EnumIter, IntoEnumIterator};
use ya_service_api::{CliCtx, CommandOutput};

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
        ConsentType::Internal => {
            "Internal Golem Network monitoring \nSecond line of info".to_string()
        }
        ConsentType::External => "External services like stats.golem.network".to_string(),
    }
}

pub fn extra_info_comment(consent_type: ConsentType) -> String {
    let info = extra_info(consent_type);
    let mut comment_info = String::new();
    for line in info.split('\n') {
        comment_info.push_str(&format!("# {}\n", line));
    }
    comment_info
}

pub fn full_question(consent_type: ConsentType) -> String {
    match consent_type {
        ConsentType::Internal => {
            "Do you agree to share data with Golem Internal Network Monitor (you can check full range of data transferred in the Terms)?".to_string()
        }
        ConsentType::External => {
            "Do you agree to share your client version, node name, node ID and wallet address, agreements statistics and payment data (available anyway on blockchain) on the stats.golem.network (External Network Monitor and Reputation System)?"
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

/// Consent management
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
    /// Show path to the consent file
    Path,
}

impl ConsentCommand {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            ConsentCommand::Show => {
                let mut values = vec![];
                for consent_type in ConsentType::iter() {
                    let allowed = have_consent(consent_type);
                    let info = extra_info(consent_type);
                    let is_allowed = match allowed {
                        Some(true) => "allow",
                        Some(false) => "deny",
                        None => "not set",
                    };
                    values.push(json!([consent_type.to_string(), is_allowed, info]));
                }
                if ctx.json_output {
                    return Ok(CommandOutput::Object(to_json()));
                }
                return Ok(CommandOutput::Table {
                    columns: ["Consent type", "Status", "Info"]
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    values,
                    summary: vec![json!(["", "", ""])],
                    header: Some(
                        "Consents given to the Golem service, you can change them, run consent --help for more info".to_string()),
                });
            }
            ConsentCommand::Allow(consent_type) => {
                set_consent(consent_type, Some(true));
            }
            ConsentCommand::Deny(consent_type) => {
                set_consent(consent_type, Some(false));
            }
            ConsentCommand::Unset(consent_type) => {
                set_consent(consent_type, None);
            }
            _ => {
                return Ok(CommandOutput::Object(json!({
                    "path": crate::api::get_consent_path().map(|p| p.to_string_lossy().to_string()).unwrap_or("not found".to_string()),
                })));
            }
        };
        Ok(CommandOutput::NoOutput)
    }
}
