use crate::api::{get_consent_path, have_consent, to_json, ConsentSource};
use crate::set_consent;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cmp::Ordering;
use std::{env, fmt};
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
    /// Consent for publication of the node's statistics
    Internal,
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
    /// Allow all types of consent (for now there is only one)
    AllowAll,
    /// Deny all types of consent (for now there is only one)
    DenyAll,
    /// Unset all types of consent (for now there is only one)
    UnsetAll,
    /// Change settings
    Allow(ConsentType),
    /// Change settings
    Deny(ConsentType),
    /// Unset setting
    Unset(ConsentType),
    /// Show path to the consent file
    Path,
}

pub fn display_consent_path() -> String {
    get_consent_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or("not found".to_string())
}

impl ConsentCommand {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            ConsentCommand::Show => {
                if ctx.json_output {
                    return Ok(CommandOutput::Object(to_json()));
                }
                let mut values = vec![];
                for consent_type in ConsentType::iter() {
                    let consent_res = have_consent(consent_type, false);
                    let info = extra_info(consent_type);
                    let is_allowed = match consent_res.consent {
                        Some(true) => "allow",
                        Some(false) => "deny",
                        None => "not set",
                    };
                    let source = match consent_res.source {
                        ConsentSource::Config => "config file".to_string(),
                        ConsentSource::Env => {
                            let env_var_name =
                                format!("YA_CONSENT_{}", &consent_type.to_string().to_uppercase());
                            format!(
                                "env variable\n({}={})",
                                &env_var_name,
                                env::var(&env_var_name).unwrap_or("".to_string())
                            )
                        }
                        ConsentSource::Default => "N/A".to_string(),
                    };
                    values.push(json!([consent_type.to_string(), is_allowed, source, info]));
                }

                return Ok(CommandOutput::Table {
                    columns: ["Type", "Status", "Source", "Info"]
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    values,
                    summary: vec![json!(["", "", "", ""])],
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
            ConsentCommand::AllowAll => {
                for consent_type in ConsentType::iter() {
                    set_consent(consent_type, Some(true));
                }
            }
            ConsentCommand::DenyAll => {
                for consent_type in ConsentType::iter() {
                    set_consent(consent_type, Some(false));
                }
            }
            ConsentCommand::UnsetAll => {
                for consent_type in ConsentType::iter() {
                    set_consent(consent_type, None);
                }
            }
            ConsentCommand::Path => {
                return Ok(CommandOutput::Object(json!({
                    "path": crate::api::get_consent_path().map(|p| p.to_string_lossy().to_string()).unwrap_or("not found".to_string()),
                })));
            }
        };
        Ok(CommandOutput::NoOutput)
    }
}
