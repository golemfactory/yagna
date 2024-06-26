use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{BufRead, Write};
use std::path::Path;
use structopt::StructOpt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConsentEntry {
    pub consent_type: ConsentType,
    pub allowed: bool,
}

#[derive(StructOpt, Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConsentType {
    Internal,
    External,
}

#[derive(StructOpt, Debug)]
pub enum ConsentCommand {
    /// Show current settings
    Show,
    /// Change settings
    Allow(ConsentType),
    /// Change settings
    Deny(ConsentType),
}

pub fn store_consent(path: &Path) {
    log::info!("Storing consent at {:?}", path);
}

pub fn save_entries(path: &Path, entries: Vec<ConsentEntry>) -> std::io::Result<()> {
    // Open the file in write-only mode
    let file = match OpenOptions::new().write(true).truncate(true).open(path) {
        Ok(file) => file,
        Err(e) => {
            log::error!("Error opening file for write: {}", e);
            return Err(e);
        }
    };
    let mut writer = io::BufWriter::new(file);

    for entry in entries {
        let entry_str = match entry.consent_type {
            ConsentType::Internal => "internal",
            ConsentType::External => "external",
        };
        let allow_str = if entry.allowed { "allow" } else { "deny" };

        writer.write_all(entry_str.as_bytes())?;
        writer.write_all(b" ")?;
        writer.write_all(allow_str.as_bytes())?;
        writer.write_all(b"\n")?;
    }

    Ok(())
}

pub fn load_entries(path: &Path) -> Vec<ConsentEntry> {
    log::info!("Loading entries from {:?}", path);

    // Open the file in read-only mode
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) => {
            log::error!("Error opening file: {}", e);
            return vec![];
        }
    };

    // Create a buffered reader
    let reader = io::BufReader::new(file);

    // Iterate over the lines in the file
    for (line_no, line) in reader.lines().enumerate() {
        let Ok(line) = line else {
            continue;
        };
        let mut entries: Vec<ConsentEntry> = Vec::new();

        if line.to_lowercase().starts_with("internal") {
            let Some(split) = line.split_once(' ') else {
                log::warn!("Invalid line: {} in file {}", line_no, path.display());
                continue;
            };
            let second_str = split.1.trim();
            if second_str.to_lowercase() == "allow" {
                entries.push(ConsentEntry {
                    consent_type: ConsentType::Internal,
                    allowed: true,
                });
            } else if second_str.to_lowercase() == "deny" {
                entries.push(ConsentEntry {
                    consent_type: ConsentType::Internal,
                    allowed: false,
                });
            } else {
                log::warn!(
                    "Error when parsing consent: No allow or deny, line: {} in file {}",
                    line_no,
                    path.display()
                );
            }
        }
    }

    vec![]
}

pub fn update_entry(path: &Path, entry: ConsentEntry) {
    log::info!("Updating entry {:?} at {:?}", entry, path);

    serde_json::to_string(&entry).unwrap();
}

#[cfg(test)]
pub fn test_save_and_load_entries() {
    let path = Path::new("test_consent.txt");
    let entries = vec![
        ConsentEntry {
            consent_type: ConsentType::Internal,
            allowed: true,
        },
        ConsentEntry {
            consent_type: ConsentType::External,
            allowed: false,
        },
    ];

    save_entries(path, entries.clone()).unwrap();
    let loaded_entries = load_entries(path);

    assert_eq!(entries, loaded_entries);
}