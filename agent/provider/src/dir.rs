use crate::startup_config::{GLOBALS_JSON, HARDWARE_JSON, PRESETS_JSON};
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
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
    let work_dir = crate::execution::exe_unit_work_dir(data_dir);
    let cache_dir = crate::execution::exe_unit_cache_dir(data_dir);
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
