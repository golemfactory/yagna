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
    pub consent_scope: ConsentScope,
    pub allowed: bool,
}

#[derive(StructOpt, Copy, Debug, Clone, Serialize, Deserialize, PartialEq, EnumIter, Eq)]
pub enum ConsentScope {
    /// Consent to augment stats.golem.network portal
    /// with data collected from your node.
    Stats,
}

impl PartialOrd for ConsentScope {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ConsentScope {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_string().cmp(&other.to_string())
    }
}

pub fn extra_info(consent_scope: ConsentScope) -> String {
    match consent_scope {
        ConsentScope::Stats => {
            "Consent to augment stats.golem.network\nportal with data collected from your node."
                .to_string()
        }
    }
}

pub fn extra_info_comment(consent_scope: ConsentScope) -> String {
    let info = extra_info(consent_scope);
    let mut comment_info = String::new();
    for line in info.split('\n') {
        comment_info.push_str(&format!("# {}\n", line));
    }
    comment_info
}

pub fn full_question(consent_scope: ConsentScope) -> String {
    match consent_scope {
        ConsentScope::Stats => {
            "Do you agree to augment stats.golem.network with data collected from your node (you can check the full range of information transferred in Terms)[allow/deny]?".to_string()
        }
    }
}

impl fmt::Display for ConsentScope {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl ConsentScope {
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
    Allow(ConsentScope),
    /// Change settings
    Deny(ConsentScope),
    /// Unset setting
    Unset(ConsentScope),
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
                for consent_scope in ConsentScope::iter() {
                    let consent_res = have_consent(consent_scope, false);
                    let info = extra_info(consent_scope);
                    let is_allowed = match consent_res.consent {
                        Some(true) => "allow",
                        Some(false) => "deny",
                        None => "not set",
                    };
                    let source = match consent_res.source {
                        ConsentSource::Config => "config file".to_string(),
                        ConsentSource::Env => {
                            let env_var_name =
                                format!("YA_CONSENT_{}", &consent_scope.to_string().to_uppercase());
                            format!(
                                "env variable\n({}={})",
                                &env_var_name,
                                env::var(&env_var_name).unwrap_or("".to_string())
                            )
                        }
                        ConsentSource::Default => "N/A".to_string(),
                    };
                    values.push(json!([consent_scope.to_string(), is_allowed, source, info]));
                }

                return Ok(CommandOutput::Table {
                    columns: ["Scope", "Status", "Source", "Info"]
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    values,
                    summary: vec![json!(["", "", "", ""])],
                    header: Some(
                        "Consents given to the Golem service, you can change them, run consent --help for more info\nSee Terms https://golem.network/privacy for details of the information collected.".to_string()),
                });
            }
            ConsentCommand::Allow(consent_scope) => {
                set_consent(consent_scope, Some(true));
            }
            ConsentCommand::Deny(consent_scope) => {
                set_consent(consent_scope, Some(false));
            }
            ConsentCommand::Unset(consent_scope) => {
                set_consent(consent_scope, None);
            }
            ConsentCommand::AllowAll => {
                for consent_scope in ConsentScope::iter() {
                    set_consent(consent_scope, Some(true));
                }
            }
            ConsentCommand::DenyAll => {
                for consent_scope in ConsentScope::iter() {
                    set_consent(consent_scope, Some(false));
                }
            }
            ConsentCommand::UnsetAll => {
                for consent_scope in ConsentScope::iter() {
                    set_consent(consent_scope, None);
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
