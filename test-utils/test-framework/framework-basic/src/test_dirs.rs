use anyhow::{anyhow, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tempdir::TempDir;
use test_binary::TestBinary;

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

fn find_binary(bin_name: &str) -> anyhow::Result<PathBuf> {
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

/// Returns path to test binary from workspace.
pub fn cargo_binary(bin_name: &str) -> anyhow::Result<PathBuf> {
    // Check if binary is already compiled.
    if let Err(_) = find_binary(bin_name) {
        TestBinary::from_workspace(&bin_name)?
            .build()
            .map_err(|e| anyhow!("Failed to compile binary: {e}"))?
            .to_str()
            .map(PathBuf::from_str)
            .ok_or(anyhow!("Failed to convert path from OsString"))??;
    };

    find_binary(bin_name)
}

/// Returns resource from `resources` directory in tests.
pub fn resource_(base_dir: &str, name: &str) -> PathBuf {
    PathBuf::from(base_dir)
        .join("tests")
        .join("resources")
        .join(name)
}

/// Generates resource from template by replacing occurrences of `${name}` pattern
/// using variables from `vars` dictionary.
/// Returns path to generated file, which is the same as `target` param, but makes it easier
/// to use this function in code.
pub fn template(
    template: &Path,
    target: impl AsRef<Path>,
    vars: &[(&str, String)],
) -> anyhow::Result<PathBuf> {
    let mut template = fs::read_to_string(&template)
        .map_err(|e| anyhow!("Loading template {} failed: {e}", template.display()))?;
    for var in vars {
        template = template.replace(&format!("${{{}}}", var.0), &var.1);
    }

    let target = target.as_ref();
    fs::write(target, template)
        .map_err(|e| anyhow!("Saving template {} failed: {e}", target.display()))?;
    Ok(target.to_path_buf())
}
