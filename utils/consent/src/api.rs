use crate::fs::{load_entries, save_entries};
use crate::model::display_consent_path;
use crate::model::{extra_info, full_question};
use crate::{ConsentCommand, ConsentEntry, ConsentScope};
use anyhow::anyhow;
use metrics::gauge;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::{env, fmt};
use structopt::lazy_static::lazy_static;
use strum::{EnumIter, IntoEnumIterator};
use ya_utils_path::data_dir::DataDir;

lazy_static! {
    static ref CONSENT_PATH: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    static ref CONSENT_CACHE: Arc<Mutex<BTreeMap<ConsentScope, ConsentEntryCached>>> =
        Arc::new(Mutex::new(BTreeMap::new()));
}

pub fn set_consent_path(path: PathBuf) {
    *CONSENT_PATH.lock() = Some(path);
}

pub fn set_consent_path_in_yagna_dir() -> anyhow::Result<()> {
    let yagna_datadir = match env::var("YAGNA_DATADIR") {
        Ok(val) => match DataDir::from_str(&val) {
            Ok(val) => val,
            Err(e) => {
                return Err(anyhow!(
                    "Problem when creating yagna path from YAGNA_DATADIR: {}",
                    e
                ))
            }
        },
        Err(_) => DataDir::new("yagna"),
    };

    let val = match yagna_datadir.get_or_create() {
        Ok(val) => val,
        Err(e) => return Err(anyhow!("Problem when creating yagna path: {}", e)),
    };

    let val = val.join("CONSENT");
    log::info!("Using yagna path: {}", val.as_path().display());
    set_consent_path(val);
    Ok(())
}

fn get_consent_env_path() -> Option<PathBuf> {
    env::var("YA_CONSENT_PATH").ok().map(PathBuf::from)
}

pub fn get_consent_path() -> Option<PathBuf> {
    let env_path = get_consent_env_path();

    // Environment path is prioritized
    if let Some(env_path) = env_path {
        return Some(env_path);
    }

    // If no environment path is set, use path setup by set_consent_path
    CONSENT_PATH.lock().clone()
}

struct ConsentEntryCached {
    consent: HaveConsentResult,
    cached_time: std::time::Instant,
}

#[derive(Copy, Debug, Clone, Serialize, Deserialize, PartialEq, EnumIter, Eq)]
pub enum ConsentSource {
    Default,
    Config,
    Env,
}
impl fmt::Display for ConsentSource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Copy, Debug, Clone)]
pub struct HaveConsentResult {
    pub consent: Option<bool>,
    pub source: ConsentSource,
}

/// Get current status of consent, it is cached for some time, so you can safely call it as much as you want
pub fn have_consent_cached(consent_scope: ConsentScope) -> HaveConsentResult {
    if cfg!(feature = "require-consent") {
        let mut map = CONSENT_CACHE.lock();

        if let Some(entry) = map.get(&consent_scope) {
            if entry.cached_time.elapsed().as_secs() < 15 {
                return entry.consent;
            }
        }
        let consent_res = have_consent(consent_scope, false);
        map.insert(
            consent_scope,
            ConsentEntryCached {
                consent: consent_res,
                cached_time: std::time::Instant::now(),
            },
        );
        gauge!(
            format!("consent.{}", consent_scope.to_lowercase_str()),
            consent_res
                .consent
                .map(|v| if v { 1 } else { 0 })
                .unwrap_or(-1) as i64
        );
        consent_res
    } else {
        // if feature require-consent is disabled, return true without checking
        HaveConsentResult {
            consent: Some(true),
            source: ConsentSource::Default,
        }
    }
}

/// Save from env is used to check if consent should be saved to configuration if set in variable
pub(crate) fn have_consent(consent_scope: ConsentScope, save_from_env: bool) -> HaveConsentResult {
    // for example:
    // YA_CONSENT_STATS=allow

    let env_variable_name = format!("YA_CONSENT_{}", consent_scope.to_string().to_uppercase());
    let result_from_env = if let Ok(env_value) = env::var(&env_variable_name) {
        if env_value.trim().to_lowercase() == "allow" {
            Some(HaveConsentResult {
                consent: Some(true),
                source: ConsentSource::Env,
            })
        } else if env_value.trim().to_lowercase() == "deny" {
            Some(HaveConsentResult {
                consent: Some(false),
                source: ConsentSource::Env,
            })
        } else {
            panic!("Invalid value for consent: {env_variable_name}={env_value}, possible values allow/deny");
        }
    } else {
        None
    };
    if let Some(result_from_env) = result_from_env {
        if save_from_env {
            //save and read again from fail
            set_consent(consent_scope, result_from_env.consent);
        } else {
            //return early with the result
            return result_from_env;
        }
    }

    let path = match get_consent_path() {
        Some(path) => path,
        None => {
            log::warn!("No consent path found");
            return HaveConsentResult {
                consent: None,
                source: ConsentSource::Default,
            };
        }
    };
    let entries = load_entries(&path);
    let mut allowed = None;
    for entry in entries {
        if entry.consent_scope == consent_scope {
            allowed = Some(entry.allowed);
        }
    }
    HaveConsentResult {
        consent: allowed,
        source: ConsentSource::Config,
    }
}

pub fn set_consent(consent_scope: ConsentScope, allowed: Option<bool>) {
    {
        CONSENT_CACHE.lock().clear();
    }
    let path = match get_consent_path() {
        Some(path) => path,
        None => {
            log::warn!("No consent path found - set consent failed");
            return;
        }
    };
    for consent_scope in ConsentScope::iter() {
        let env_name = format!("YA_CONSENT_{}", consent_scope.to_string().to_uppercase());
        if let Ok(env_val) = env::var(&env_name) {
            log::warn!(
                "Consent {} is already set by environment variable, changes to configuration may not have effect: {}={}",
                consent_scope,
                env_name,
                env_val)
        }
    }
    let mut entries = load_entries(&path);
    entries.retain(|entry| entry.consent_scope != consent_scope);
    if let Some(allowed) = allowed {
        entries.push(ConsentEntry {
            consent_scope,
            allowed,
        });
    }
    entries.sort_by(|a, b| a.consent_scope.cmp(&b.consent_scope));
    match save_entries(&path, entries) {
        Ok(_) => log::info!("Consent saved: {} {:?}", consent_scope, allowed),
        Err(e) => log::error!("Error when saving consent: {}", e),
    }
}

pub fn to_json() -> serde_json::Value {
    json!({
        "consents": ConsentScope::iter()
            .map(|consent_scope: ConsentScope| {
                let consent_res = have_consent(consent_scope, false);
                let consent = match consent_res.consent {
                    Some(true) => "allow",
                    Some(false) => "deny",
                    None => "not set",
                };
                let source_location = match consent_res.source {
                    ConsentSource::Config => display_consent_path(),
                    ConsentSource::Env => {
                        let env_var_name = format!("YA_CONSENT_{}", &consent_scope.to_string().to_uppercase());
                        format!("({}={})", &env_var_name, env::var(&env_var_name).unwrap_or("".to_string()))
                    },
                    ConsentSource::Default => "N/A".to_string(),
                };
                json!({
                    "type": consent_scope.to_string(),
                    "consent": consent,
                    "source": consent_res.source.to_string(),
                    "location": source_location,
                    "info": extra_info(consent_scope),
                    "question": full_question(consent_scope),
                })
            })
            .collect::<Vec<_>>()
    })
}

pub fn run_consent_command(consent_command: ConsentCommand) {
    match consent_command {
        ConsentCommand::Show => {
            println!(
                "{}",
                serde_json::to_string_pretty(&to_json()).expect("json serialization failed")
            );
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
            println!(
                "{}",
                get_consent_path()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or("not found".to_string())
            )
        }
    }
}
