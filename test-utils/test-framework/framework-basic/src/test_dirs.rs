use anyhow::{anyhow, bail};
use std::fs;
use std::path::PathBuf;
use tempdir::TempDir;

pub mod macros {
    /// Creates temporary directory in cargo target directory.
    #[macro_export]
    macro_rules! temp_dir {
        ($prefix:literal) => {
            // CARGO_TARGET_TMPDIR is available in compile time only in binary modules, so we can't
            // use it in this library. Thanks to using macro it will be resolved in final code not here
            // and it will work.
            ya_framework_basic::test_dirs::temp_dir_(env!("CARGO_TARGET_TMPDIR"), $prefix)
        };
    }

    /// Returns resource from `resources` directory in tests folder.
    #[macro_export]
    macro_rules! resource {
        ($name:literal) => {
            // CARGO_MANIFEST_DIR is available in compile time only in binary modules, so we can't
            // use it in this library. Thanks to using macro it will be resolved in final code not here
            // and it will work.
            ya_framework_basic::test_dirs::resource_(env!("CARGO_MANIFEST_DIR"), $name)
        };
    }
}

pub fn temp_dir_(base_dir: &str, prefix: &str) -> anyhow::Result<TempDir> {
    fs::create_dir_all(base_dir)?;
    let dir = TempDir::new_in(base_dir, prefix)?;
    let temp_dir = dir.path();
    fs::create_dir_all(temp_dir)?;

    Ok(dir)
}

#[cfg(debug_assertions)]
pub fn is_debug() -> bool {
    true
}

#[cfg(not(debug_assertions))]
pub fn is_debug() -> bool {
    false
}

#[cfg(target_family = "windows")]
pub fn extension() -> String {
    ".exe".to_string()
}

#[cfg(not(target_family = "windows"))]
pub fn extension() -> String {
    "".to_string()
}

/// Returns absolute path to cargo project binary.
pub fn cargo_binary(bin_name: &str) -> anyhow::Result<PathBuf> {
    let current = std::env::current_exe()
        .map_err(|e| anyhow!("Failed to get path to current binary. {e}"))?
        .parent()
        .and_then(|path| path.parent())
        .ok_or(anyhow!("No parent dir for current binary."))?
        .to_path_buf();
    let bin_name = format!("{bin_name}{}", extension());
    let bin_path = current.join(&bin_name);
    if !bin_path.exists() {
        bail!(
            "Path doesn't exist: {}, when looking for binary: {}",
            bin_path.display(),
            bin_name
        );
    }

    if !bin_path.is_file() {
        bail!("Expected {} to be binary file.", bin_path.display());
    }

    Ok(bin_path)
}

/// Returns resource from `resources` directory in tests.
pub fn resource_(base_dir: &str, name: &str) -> PathBuf {
    PathBuf::from(base_dir)
        .join("tests")
        .join("resources")
        .join(name)
}
