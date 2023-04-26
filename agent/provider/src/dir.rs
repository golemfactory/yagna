use crate::startup_config::{GLOBALS_JSON, HARDWARE_JSON, PRESETS_JSON};
use anyhow::{bail, Result};
use std::path::Path;
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

pub fn clean_provider_dir<P: AsRef<Path>, S: AsRef<str>>(
    dir: P,
    expr: S,
    check_dir: bool,
    dry_run: bool,
) -> Result<u64> {
    let lifetime = humantime::parse_duration(expr.as_ref())?;
    if check_dir && !is_provider_dir(&dir)? {
        bail!("Not a provider data directory: {}", dir.as_ref().display());
    }
    Ok(clean_data_dir(dir, lifetime, dry_run))
}

fn is_provider_dir<P: AsRef<Path>>(dir: P) -> Result<bool> {
    let mut files = vec![
        (HARDWARE_JSON, false),
        (PRESETS_JSON, false),
        (GLOBALS_JSON, false),
    ];

    dir.as_ref()
        .read_dir()?
        .filter_map(|r| r.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .for_each(|name| {
            for (n, v) in files.iter_mut() {
                if name == *n {
                    *v = true;
                }
            }
        });

    Ok(files.iter().all(|pair| pair.1))
}

fn clean_data_dir<P: AsRef<Path>>(data_dir: P, lifetime: Duration, dry_run: bool) -> u64 {
    let work_dir = crate::execution::exe_unit_work_dir(&data_dir);
    let cache_dir = crate::execution::exe_unit_cache_dir(&data_dir);
    clean_dir(work_dir, lifetime, dry_run) + clean_dir(cache_dir, lifetime, dry_run)
}

fn clean_dir<P: AsRef<Path>>(dir: P, lifetime: Duration, dry_run: bool) -> u64 {
    let mut dirs = Vec::new();
    let deadline = SystemTime::now() - lifetime;

    let total_bytes = WalkDir::new(dir.as_ref())
        .into_iter()
        .filter_map(|result| result.ok())
        .filter_map(|entry| match entry.metadata() {
            Ok(meta) => Some((entry.path().to_owned(), meta)),
            _ => None,
        })
        .inspect(|path_meta| {
            if path_meta.1.is_dir() {
                dirs.push(path_meta.0.clone());
            }
        })
        .filter(|path_meta| {
            path_meta.1.is_file()
                && match path_meta.1.modified() {
                    Ok(sys_time) => sys_time <= deadline,
                    _ => false,
                }
        })
        .fold(0, |acc, path_meta| {
            if !dry_run && std::fs::remove_file(&path_meta.0).is_err() {
                return acc;
            }
            acc + path_meta.1.len()
        });

    if !dry_run {
        dirs.sort_by_key(|path| path.components().count());
        dirs.into_iter().rev().for_each(|path| {
            if let Ok(mut contents) = path.read_dir() {
                if contents.next().is_none() {
                    let _ = std::fs::remove_dir_all(path);
                }
            }
        });
    }

    total_bytes
}

#[cfg(test)]
mod tests {
    use std::{fs::File, path::PathBuf};
    use crate::startup_config::{HARDWARE_JSON, PRESETS_JSON, GLOBALS_JSON};

    use super::clean_provider_dir;

    #[test]
    fn test_empty_dir_fail() {
        let dir = tempfile::tempdir().unwrap().into_path();
        let expected = anyhow::anyhow!("Not a provider data directory: {}", dir.display());
        let actual = clean_provider_dir(dir, "1d", true, false);
        assert_eq!(expected.to_string(), actual.err().unwrap().to_string());
    }

    #[test]
    fn test_data_dir_cleanup() {
        let data_dir = create_data_dir(); 
        let work_dir = crate::execution::exe_unit_work_dir(&data_dir);
        std::fs::create_dir(work_dir);
        let cache_dir = crate::execution::exe_unit_cache_dir(&data_dir);
        std::fs::create_dir(cache_dir);

        let actual = clean_provider_dir(data_dir, "1d", true, false).expect("Is ok and returns 0");
        assert_eq!(0, actual);
    }

    fn create_data_dir() -> PathBuf {
        let data_dir = tempfile::tempdir().unwrap().into_path();
        create_file(&data_dir, HARDWARE_JSON);
        create_file(&data_dir, PRESETS_JSON);
        create_file(&data_dir, GLOBALS_JSON); 
        data_dir
    }

    fn create_file(dir: &PathBuf, file_name: &str) {
        let file_path = dir.join(file_name);
        File::create(file_path).unwrap();
    }
}
