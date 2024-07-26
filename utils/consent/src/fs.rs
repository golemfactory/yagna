use crate::parser::{entries_to_str, str_to_entries};
use crate::ConsentEntry;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{Read, Write};
use std::path::Path;

pub fn save_entries(path: &Path, entries: Vec<ConsentEntry>) -> std::io::Result<()> {
    let file_exists = path.exists();
    // Open the file in write-only mode
    let file = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
    {
        Ok(file) => file,
        Err(e) => {
            log::error!("Error opening file for write: {}", e);
            return Err(e);
        }
    };
    if file_exists {
        log::info!("Overwriting consent file: {}", path.display());
    } else {
        log::info!("Created consent file: {}", path.display());
    }
    let mut writer = io::BufWriter::new(file);

    writer.write_all(entries_to_str(entries).as_bytes())
}

pub fn load_entries(path: &Path) -> Vec<ConsentEntry> {
    log::debug!("Loading entries from {:?}", path);

    let str = {
        if !path.exists() {
            log::info!("Consent file not exist: {}", path.display());
            return vec![];
        }
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
            log::error!(
                "File unreasonably large, skipping parsing: {}",
                path.display()
            );
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
                log::error!(
                    "Error when decoding file (wrong binary format): {} {}",
                    path.display(),
                    e
                );
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
        log::info!("Fixing consent file: {}", path.display());
        match OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
        {
            Ok(file) => {
                let mut writer = io::BufWriter::new(file);

                match writer.write_all(str_entries.as_bytes()) {
                    Ok(_) => (),
                    Err(e) => {
                        log::error!("Error writing to file: {} {}", path.display(), e);
                    }
                }
            }
            Err(e) => {
                log::error!("Error opening file for write: {}", e);
            }
        };
    } else {
        log::debug!("Consent file doesn't need fixing: {}", path.display());
    }

    entries
}

#[test]
pub fn test_entries_internal() {
    use crate::ConsentType;
    use rand::Rng;
    use std::path::PathBuf;
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "debug");
    }
    let rand_string: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();

    env_logger::init();
    let path = PathBuf::from(format!("tmp-{}.txt", rand_string));
    let entries = vec![ConsentEntry {
        consent_type: ConsentType::Internal,
        allowed: true,
    }];

    save_entries(&path, entries.clone()).unwrap();
    let loaded_entries = load_entries(&path);

    assert_eq!(entries, loaded_entries);
    std::fs::remove_file(&path).unwrap();
}
