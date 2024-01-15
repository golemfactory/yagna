use std::fs;
use tempdir::TempDir;

pub mod macros {
    #[macro_export]
    macro_rules! temp_dir {
        ($prefix:literal) => {
            // CARGO_TARGET_TMPDIR is available in compile time only in binary modules, so we can't
            // use it in this library. Thanks to using macro it will be resolved in final code not here
            // and it will work.
            ya_framework_basic::temp::temp_dir_(env!("CARGO_TARGET_TMPDIR"), $prefix)
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
