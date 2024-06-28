use crate::fs::{load_entries, save_entries};
use crate::{ConsentCommand, ConsentEntry, ConsentType};
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use anyhow::bail;
use structopt::lazy_static::lazy_static;
use ya_utils_path::data_dir::DataDir;

lazy_static! {
    static ref CONSENT_PATH: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
}

pub fn set_consent_path(path: PathBuf) {
    *CONSENT_PATH.lock().expect("lock_cannot_fail") = Some(path);
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

fn get_consent_path() -> Option<PathBuf> {
    let env_path = get_consent_env_path();

    // Environment path is prioritized
    if let Some(env_path) = env_path {
        return Some(env_path);
    }

    // If no environment path is set, use path setup by set_consent_path
    CONSENT_PATH.lock().expect("lock_cannot_fail").clone()
}

pub fn have_consent(consent_type: ConsentType) -> Option<bool> {
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
    entries.sort_by(|a, b| a.consent_type.to_string().cmp(&b.consent_type.to_string()));
    match save_entries(&path, entries) {
        Ok(_) => log::info!("Consent saved: {} {:?}", consent_type, allowed),
        Err(e) => log::error!("Error when saving consent: {}", e),
    }
}

pub fn run_consent_command(consent_command: ConsentCommand) {
    match consent_command {
        ConsentCommand::Show => {}
        ConsentCommand::Allow(consent_type) => {
            set_consent(consent_type, Some(true));
        }
        ConsentCommand::Deny(consent_type) => {
            set_consent(consent_type, Some(false));
        }
        ConsentCommand::Unset(consent_type) => {
            set_consent(consent_type, None);
        }
    }
}
