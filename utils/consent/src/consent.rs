use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::{fmt, io};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use structopt::StructOpt;
use strum::{EnumIter, IntoEnumIterator};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConsentEntry {
    pub consent_type: ConsentType,
    pub allowed: bool,
}

#[derive(StructOpt, Debug, Clone, Serialize, Deserialize, PartialEq, EnumIter)]
pub enum ConsentType {
    /// Internal consent
    Internal,
    /// External consent
    External,
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

pub fn save_entries(path: &Path, entries: Vec<ConsentEntry>) -> std::io::Result<()> {
    // Open the file in write-only mode
    let file = match OpenOptions::new().create(true).write(true).truncate(true).open(path) {
        Ok(file) => file,
        Err(e) => {
            log::error!("Error opening file for write: {}", e);
            return Err(e);
        }
    };
    let mut writer = io::BufWriter::new(file);

    writer.write_all(entries_to_str(entries).as_bytes())
}

pub fn str_to_entries(str: &str, err_decorator_path: String) -> Vec<ConsentEntry> {
    let mut entries_map: BTreeMap<String, ConsentEntry> = BTreeMap::new();
    // Iterate over the lines in the file

    'outer: for (line_no, line) in str.split('\n').enumerate() {
        let line = line.trim().to_lowercase();
        log::debug!("Reading line: {}, {}", line_no, line);
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
            log::warn!(
                "Error when parsing consent: Invalid line: {} in file {}",
                line_no,
                err_decorator_path
            );
        }
    }

    let mut entries: Vec<ConsentEntry> = Vec::new();
    for (_, entry) in entries_map {
        entries.push(entry);
    }
    entries
}

pub fn load_entries(path: &Path) -> Vec<ConsentEntry> {
    log::info!("Loading entries from {:?}", path);

    let str = {
        // Open the file in read-only mode
        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) => {
                log::error!("Error opening file: {} {}", path.display(), e);
                return vec![];
            }
        };


        let file_len = match file.metadata() {
            Ok(metadata) => metadata.len(),
            Err(e) => {
                log::error!("Error reading file metadata: {} {}", path.display(), e);
                return vec![];
            }
        };

        if file_len > 100000 {
            log::error!("File unreasonably large, skipping parsing: {}", path.display());
            return vec![];
        }

        let mut reader = io::BufReader::new(file);

        let mut buf = vec![0; file_len as usize];

        match reader.read_exact(&mut buf) {
            Ok(_) => (),
            Err(e) => {
                log::error!("Error reading file: {} {}", path.display(), e);
                return vec![];
            }
        }
        match String::from_utf8(buf) {
            Ok(str) => str,
            Err(e) => {
                log::error!("Error when decoding file (wrong binary format): {} {}", path.display(), e);
                return vec![];
            }
        }
    };

    let entries = str_to_entries(&str, path.display().to_string());

    log::debug!("Loaded entries: {:?}", entries);
    // normalize entries file
    let str_entries = entries_to_str(entries.clone());
    let entries2 = str_to_entries(&str_entries, "internal".to_string());

    if entries2 != entries {
        log::warn!("Internal problem when normalizing entries file");
        return entries;
    }

    if str_entries != str {
        log::debug!("Detected difference in consent file, writing to file");
        match OpenOptions::new().create(true).write(true).truncate(true).open(path) {
            Ok(file) => {
                let mut writer = io::BufWriter::new(file);

                match writer.write_all(str_entries.as_bytes()) {
                    Ok(_) => (),
                    Err(e) => {
                        log::error!("Error writing to file: {} {}", path.display(), e);
                    }
                }
            },
            Err(e) => {
                log::error!("Error opening file for write: {}", e);
            }
        };
    } else {
        log::debug!("No difference in consent file - no additional write needed");
    }

    entries
}

pub fn update_entry(path: &Path, entry: ConsentEntry) {
    log::info!("Updating entry {:?} at {:?}", entry, path);

 //   serde_json::to_string(&entry).unwrap();
}
