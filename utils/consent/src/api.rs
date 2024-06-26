use std::path::Path;
use crate::consent::{load_entries, save_entries};
use crate::{ConsentEntry, ConsentType};

pub fn have_consent(consent_type: ConsentType) -> Option<bool> {
    let path = Path::new("test_consent.txt");
    let entries = load_entries(path);
    let mut allowed = false;
    for entry in entries {
        if entry.consent_type == consent_type {
            allowed = entry.allowed;
        }
    }
    Some(allowed)
}

pub fn set_consent(consent_type: ConsentType, allowed:bool) {
    let path = Path::new("test_consent.txt");
    let mut entries = load_entries(path);
    entries.retain(|entry| entry.consent_type != consent_type);
    entries.push(ConsentEntry {
        consent_type,
        allowed,
    });
    entries.sort_by(|a, b| a.consent_type.to_string().cmp(&b.consent_type.to_string()));
    match save_entries(path, entries) {
        Ok(_) => log::info!("Consent saved"),
        Err(e) => log::error!("Error when saving consent: {}", e),
    }
}