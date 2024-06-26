use std::fs::{File, OpenOptions};
use std::{io};
use std::io::{Read, Write};
use std::path::Path;
use crate::{ConsentEntry};
use crate::parser::{entries_to_str, str_to_entries};


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


#[test]
pub fn test_save_and_load_entries() {
    use crate::ConsentType;
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "debug");
    }
    env_logger::init();
    let path = Path::new("test_consent.txt");
    let entries = vec![
        ConsentEntry {
            consent_type: ConsentType::External,
            allowed: false,
        },
        ConsentEntry {
            consent_type: ConsentType::Internal,
            allowed: true,
        }
    ];

    save_entries(path, entries.clone()).unwrap();
    let loaded_entries = load_entries(path);

    assert_eq!(entries, loaded_entries);
}
