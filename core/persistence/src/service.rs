#![allow(clippy::ptr_arg)]

use std::path::{Path, PathBuf};
use structopt::StructOpt;

use ya_service_api::{CliCtx, CommandOutput};
use ya_service_api_interfaces::{Provider, Service};
use ya_utils_process::lock::ProcLock;

use crate::executor::DbExecutor;

/// Persistence service
pub struct Persistence;

impl Service for Persistence {
    type Cli = Command;
}

impl Persistence {
    /// Run DB vacuum on startup
    pub async fn gsb<Context: Provider<Self, CliCtx>>(context: &Context) -> anyhow::Result<()> {
        let ctx = context.component();
        vacuum(&ctx.data_dir, filter::wal_larger_than_db, true).await?;
        Ok(())
    }
}

/// Database management
#[derive(StructOpt, Debug)]
pub enum Command {
    /// Rebuild databases to reduce size
    #[structopt(setting = structopt::clap::AppSettings::DeriveDisplayOrder)]
    Vacuum {
        /// Vacuum when the daemon is running
        #[structopt(long)]
        force: bool,
    },
}

impl Command {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            Command::Vacuum { force } => vacuum(&ctx.data_dir, filter::any, force).await,
        }
    }
}

async fn vacuum<F, P>(data_dir: P, filter: F, force: bool) -> anyhow::Result<CommandOutput>
where
    F: Fn(&PathBuf) -> bool,
    P: AsRef<Path>,
{
    let db_files = std::fs::read_dir(&data_dir)?
        .filter_map(|r| r.map(|e| e.path()).ok())
        .filter(|p| !p.is_dir())
        .filter(|p| {
            p.extension()
                .map(|e| {
                    let ext = e.to_string_lossy().to_lowercase();
                    ext.as_str() == "db"
                })
                .unwrap_or(false)
        })
        .filter(filter)
        .collect::<Vec<_>>();

    if db_files.is_empty() {
        return Ok(CommandOutput::Object(serde_json::Value::String(
            "no databases found to vacuum".to_string(),
        )));
    }

    if !force && ProcLock::contains_locks(&data_dir)? {
        anyhow::bail!(
            "Data directory '{}' is used by another application. Use '--force' to override.",
            data_dir.as_ref().display()
        );
    }

    for db_file in db_files {
        eprintln!("vacuuming {}", db_file.display());
        let db = DbExecutor::new(db_file.display().to_string())?;
        db.execute("VACUUM;").await?;
    }

    Ok(CommandOutput::NoOutput)
}

mod filter {
    use std::path::PathBuf;

    pub(super) fn any(_: &PathBuf) -> bool {
        true
    }

    pub(super) fn wal_larger_than_db(db: &PathBuf) -> bool {
        let mut wal = db.to_path_buf();
        wal.set_extension("db-wal");

        let db_meta = match db.metadata() {
            Ok(meta) => meta,
            _ => return false,
        };
        let wal_meta = match wal.metadata() {
            Ok(meta) => meta,
            _ => return false,
        };

        wal_meta.len() > db_meta.len()
    }
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::path::Path;

    use ya_service_api::CommandOutput;
    use ya_utils_process::lock::ProcLock;

    use crate::service::filter;
    use crate::service::vacuum;

    fn touch_db<P: AsRef<Path>>(path: P, name: &str) -> anyhow::Result<()> {
        OpenOptions::new()
            .write(true)
            .create(true)
            .open(path.as_ref().join(format!("{}.db", name)))?;
        Ok(())
    }

    #[tokio::test]
    async fn vacuum_dir() -> anyhow::Result<()> {
        let temp_dir = tempdir::TempDir::new("vacuum")?;
        let temp_path = temp_dir.path();

        touch_db(&temp_path, "test")?;

        assert!(vacuum(&temp_path, filter::any, false).await.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn vacuum_locked_dir() -> anyhow::Result<()> {
        let temp_dir = tempdir::TempDir::new("vacuum")?;
        let temp_path = temp_dir.path();

        touch_db(&temp_path, "test")?;

        let _lock = ProcLock::new("temp", &temp_path)?.lock(std::process::id())?;
        assert!(vacuum(&temp_path, filter::any, false).await.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn vacuum_locked_dir_forced() -> anyhow::Result<()> {
        let temp_dir = tempdir::TempDir::new("vacuum")?;
        let temp_path = temp_dir.path();

        touch_db(&temp_path, "test")?;

        let _lock = ProcLock::new("temp", &temp_path)?.lock(std::process::id())?;
        assert!(vacuum(&temp_path, filter::any, true).await.is_ok());

        Ok(())
    }

    #[tokio::test]
    async fn vacuum_when() -> anyhow::Result<()> {
        let temp_dir = tempdir::TempDir::new("vacuum")?;
        let temp_path = temp_dir.path();

        touch_db(&temp_path, "test")?;

        match vacuum(&temp_path, |_| false, true).await? {
            CommandOutput::Object(_) => (),
            _ => panic!("invalid result"),
        }

        match vacuum(&temp_path, filter::any, true).await? {
            CommandOutput::NoOutput => (),
            _ => panic!("invalid result"),
        }

        Ok(())
    }
}
