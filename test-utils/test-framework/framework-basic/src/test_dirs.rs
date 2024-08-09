use anyhow::anyhow;
use std::fs;
use std::path::{Path, PathBuf};
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
    let mut template = fs::read_to_string(template)
        .map_err(|e| anyhow!("Loading template {} failed: {e}", template.display()))?;
    for var in vars {
        template = template.replace(&format!("${{{}}}", var.0), &var.1);
    }

    let target = target.as_ref();
    fs::write(target, template)
        .map_err(|e| anyhow!("Saving template {} failed: {e}", target.display()))?;
    Ok(target.to_path_buf())
}
