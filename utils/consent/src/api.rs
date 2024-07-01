use crate::fs::{load_entries, save_entries};
use crate::model::{extra_info, full_question};
use crate::{ConsentCommand, ConsentEntry, ConsentType};
use anyhow::bail;
use parking_lot::Mutex;
use serde_json::json;
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use structopt::lazy_static::lazy_static;
use strum::IntoEnumIterator;
use ya_utils_path::data_dir::DataDir;

lazy_static! {
    static ref CONSENT_PATH: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    static ref CONSENT_CACHE: Arc<Mutex<BTreeMap<ConsentType, ConsentEntryCached>>> =
        Arc::new(Mutex::new(BTreeMap::new()));
}

pub fn set_consent_path(path: PathBuf) {
    *CONSENT_PATH.lock() = Some(path);
}

pub fn set_consent_path_in_yagna_dir() -> anyhow::Result<()> {
    let yagna_datadir = match env::var("YAGNA_DATADIR") {
        Ok(val) => val,
        Err(_) => "yagna".to_string(),
    };
    let val = match DataDir::new(&yagna_datadir).get_or_create() {
        Ok(val) => val,
        Err(e) => {
            bail!("Problem when creating yagna path: {}", e)
        }
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
    consent: Option<bool>,
    cached_time: std::time::Instant,
}

/// Get current status of consent, it is cached for some time, so you can safely call it as much as you want
pub fn have_consent_cached(consent_type: ConsentType) -> Option<bool> {
    if cfg!(feature = "require-consent") {
        let mut map = CONSENT_CACHE.lock();

        if let Some(entry) = map.get(&consent_type) {
            if entry.cached_time.elapsed().as_secs() < 15 {
                return entry.consent;
            }
        }
        let consent = have_consent(consent_type);
        map.insert(
            consent_type,
            ConsentEntryCached {
                cached_time: std::time::Instant::now(),
                consent,
            },
        );
        consent
    } else {
        // if feature require-consent is enabled, return true without checking
        Some(true)
    }
}

pub(crate) fn have_consent(consent_type: ConsentType) -> Option<bool> {
    // for example:
    // YA_CONSENT_INTERNAL=allow
    // YA_CONSENT_EXTERNAL=deny
    if let Ok(env_value) = env::var(format!(
        "YA_CONSENT_{}",
        consent_type.to_string().to_uppercase()
    )) {
        if env_value.trim().to_lowercase() == "allow" {
            return Some(true);
        } else if env_value.trim().to_lowercase() == "deny" {
            return Some(false);
        } else {
            panic!("Invalid value for consent: {}", env_value);
        }
    }

    let path = match get_consent_path() {
        Some(path) => path,
        None => {
            log::warn!("No consent path found");
            return None;
        }
    };
    let entries = load_entries(&path);
    let mut allowed = None;
    for entry in entries {
        if entry.consent_type == consent_type {
            allowed = Some(entry.allowed);
        }
    }
    allowed
}

pub fn set_consent(consent_type: ConsentType, allowed: Option<bool>) {
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
    let mut entries = load_entries(&path);
    entries.retain(|entry| entry.consent_type != consent_type);
    if let Some(allowed) = allowed {
        entries.push(ConsentEntry {
            consent_type,
            allowed,
        });
    }
    entries.sort_by(|a, b| a.consent_type.cmp(&b.consent_type));
    match save_entries(&path, entries) {
        Ok(_) => log::info!("Consent saved: {} {:?}", consent_type, allowed),
        Err(e) => log::error!("Error when saving consent: {}", e),
    }
}

pub fn to_json() -> serde_json::Value {
    json!({
        "consents": ConsentType::iter()
            .map(|consent_type: ConsentType| {
                let consent = match have_consent(consent_type) {
                    Some(true) => "allow",
                    Some(false) => "deny",
                    None => "not set",
                };
                json!({
                    "type": consent_type.to_string(),
                    "consent": consent,
                    "info": extra_info(consent_type),
                    "question": full_question(consent_type),
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
            println!(
                "{}",
                get_consent_path()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or("not found".to_string())
            )
        }
    }
}
