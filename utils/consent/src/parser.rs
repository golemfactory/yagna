use std::collections::BTreeMap;
use strum::IntoEnumIterator;
use crate::{ConsentEntry, ConsentType};

pub fn entries_to_str(entries: Vec<ConsentEntry>) -> String {
    let mut res = String::new();
    res.push_str("# This file contains consent settings\n");
    res.push_str("# Format: <consent_type> <allow|deny>\n");

    for entry in entries {
        let allow_str = if entry.allowed { "allow" } else { "deny" };
        res.push_str(&format!("{} {}\n", entry.consent_type, allow_str));
    }
    res
}

pub fn str_to_entries(str: &str, err_decorator_path: String) -> Vec<ConsentEntry> {
    let mut entries_map: BTreeMap<String, ConsentEntry> = BTreeMap::new();
    // Iterate over the lines in the file

    'outer: for (line_no, line) in str.split('\n').enumerate() {
        let line = line.trim().to_lowercase();
        if line.starts_with('#') {
            continue;
        }
        if line.is_empty() {
            continue;
        }
        for consent_type in ConsentType::iter() {
            let consent_type_str = consent_type.to_lowercase_str();
            if line.starts_with(&consent_type_str) {
                let Some(split) = line.split_once(' ') else {
                    log::warn!("Invalid line: {} in file {}", line_no, err_decorator_path);
                    continue 'outer;
                };
                let second_str = split.1.trim();

                let allowed = if second_str == "allow" {
                    true
                } else if second_str == "deny" {
                    false
                } else {
                    log::warn!(
                        "Error when parsing consent: No allow or deny, line: {} in file {}",
                        line_no,
                        err_decorator_path
                    );
                    continue 'outer;
                };
                if let Some(entry) = entries_map.get_mut(&consent_type_str) {
                    if entry.allowed != allowed {
                        log::warn!(
                            "Error when parsing consent: Duplicate entry with different value, line: {} in file {}",
                            line_no,
                            err_decorator_path
                        );
                    }
                } else {
                    let entry = ConsentEntry {
                        consent_type,
                        allowed,
                    };
                    entries_map.insert(consent_type_str, entry);
                }
                continue 'outer;
            }
        }
        log::warn!(
                "Error when parsing consent: Invalid line: {} in file {}",
                line_no,
                err_decorator_path
            );
    }

    let mut entries: Vec<ConsentEntry> = Vec::new();
    for (_, entry) in entries_map {
        entries.push(entry);
    }
    entries
}
