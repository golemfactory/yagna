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
        .min_depth(1)
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
    use super::clean_provider_dir;
    use crate::startup_config::{GLOBALS_JSON, HARDWARE_JSON, PRESETS_JSON};
    use std::{fs::File, io::Write, path::PathBuf};

    #[test]
    fn test_empty_dir_fail() {
        let dir = tempfile::tempdir().unwrap().into_path();
        let expected = anyhow::anyhow!("Not a provider data directory: {}", dir.display());
        let error = clean_provider_dir(&dir, "1d", true, false);
        assert_eq!(expected.to_string(), error.err().unwrap().to_string());
        assert!(dir.exists());
    }

    #[test]
    fn test_empty_exe_unit_dir_cleanup() {
        let dirs = create_data_dir_w_exe_unit();
        let removed_bytes =
            clean_provider_dir(dirs.data_dir.dir, "1d", true, false).expect("Is ok");

        assert_eq!(0, removed_bytes);
        assert!(dirs.data_dir.globals_json.exists());
        assert!(dirs.data_dir.hardware_json.exists());
        assert!(dirs.data_dir.presets_json.exists());
        assert!(dirs.work_dir.exists());
        assert!(dirs.cache_dir.exists());
    }

    #[test]
    fn test_exe_unit_dir_cleanup_of_old_files() {
        let dirs = create_data_dir_w_exe_unit();
        let work_dir_file = create_file(&dirs.work_dir, "a.txt", "a");
        let work_dir_dir = create_dir(&dirs.work_dir, "a");
        let cache_dir_file = create_file(&dirs.cache_dir, "b.txt", "b");
        let cache_dir_dir = create_dir(&dirs.cache_dir, "b");
        let removed_bytes: u64 =
            clean_provider_dir(dirs.data_dir.dir, "0us", true, false).expect("Is ok");

        assert_eq!(2, removed_bytes);
        assert!(dirs.data_dir.globals_json.exists());
        assert!(dirs.data_dir.hardware_json.exists());
        assert!(dirs.data_dir.presets_json.exists());
        assert!(dirs.work_dir.exists());
        assert!(dirs.cache_dir.exists());
        assert!(!work_dir_dir.exists());
        assert!(!work_dir_file.exists());
        assert!(!cache_dir_dir.exists());
        assert!(!cache_dir_file.exists());
    }

    #[test]
    fn test_exe_unit_dir_cleanup_does_not_remove_files() {
        let dirs = create_data_dir_w_exe_unit();
        let work_dir_file = create_file(&dirs.work_dir, "a.txt", "a");
        let work_dir_dir = create_dir(&dirs.work_dir, "a");
        let cache_dir_file = create_file(&dirs.cache_dir, "b.txt", "b");
        let cache_dir_dir = create_dir(&dirs.cache_dir, "b");
        let cache_dir_dir_file = create_file(&cache_dir_dir, "c.txt", "c");
        let removed_bytes: u64 =
            clean_provider_dir(dirs.data_dir.dir, "1h", true, false).expect("Is ok");

        assert_eq!(0, removed_bytes);
        assert!(dirs.data_dir.globals_json.exists());
        assert!(dirs.data_dir.hardware_json.exists());
        assert!(dirs.data_dir.presets_json.exists());
        assert!(dirs.work_dir.exists());
        assert!(dirs.cache_dir.exists());
        assert!(
            !work_dir_dir.exists(),
            "Empty directories removed even when not expired"
        );
        assert!(work_dir_file.exists());
        assert!(cache_dir_dir.exists());
        assert!(cache_dir_file.exists());
        assert!(cache_dir_dir_file.exists());
    }

    struct DataDir {
        dir: PathBuf,
        hardware_json: PathBuf,
        presets_json: PathBuf,
        globals_json: PathBuf,
    }

    struct ExeUnitDirs {
        data_dir: DataDir,
        cache_dir: PathBuf,
        work_dir: PathBuf,
    }

    fn create_data_dir_w_exe_unit() -> ExeUnitDirs {
        let data_dir = create_data_dir();
        let work_dir = crate::execution::exe_unit_work_dir(&data_dir.dir);
        let _ = std::fs::create_dir_all(&work_dir).unwrap();
        let cache_dir = crate::execution::exe_unit_cache_dir(&data_dir.dir);
        let _ = std::fs::create_dir_all(&cache_dir).unwrap();
        ExeUnitDirs {
            data_dir,
            cache_dir,
            work_dir,
        }
    }

    fn create_data_dir() -> DataDir {
        let dir = tempfile::tempdir().unwrap().into_path();
        let hardware_json = create_file(&dir, HARDWARE_JSON, "a");
        let presets_json = create_file(&dir, PRESETS_JSON, "b");
        let globals_json = create_file(&dir, GLOBALS_JSON, "c");
        DataDir {
            dir,
            hardware_json,
            presets_json,
            globals_json,
        }
    }

    fn create_dir(dir: &PathBuf, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn create_file(dir: &PathBuf, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        let mut file = File::create(&path).unwrap();
        if !content.is_empty() {
            file.write_all(content.as_bytes()).unwrap();
        }
        path
    }
}
