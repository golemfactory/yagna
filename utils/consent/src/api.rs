use std::env;
use std::path::{PathBuf};
use crate::consent::{load_entries, save_entries};
use crate::{ConsentEntry, ConsentType};

pub fn get_consent_default_path() -> Option<PathBuf> {
    env::var("YA_CONSENT_PATH").ok().map(PathBuf::from)
}

pub fn get_consent_path() -> Option<PathBuf> {
    get_consent_default_path()
}

pub fn have_consent(consent_type: ConsentType) -> Option<bool> {
    let path = match get_consent_path() {
        Some(path) => path,
        None => {
            log::warn!("No consent path found");
            return None;
        },
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
        },
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