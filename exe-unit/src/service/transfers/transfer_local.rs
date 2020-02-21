use anyhow::{Result, Error, Context};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use url::Url;

use super::TransferProtocol;


pub struct LocalTransfer;


impl TransferProtocol for LocalTransfer {
    fn transfer(&self, from: &Url, to: &Url) -> Result<()> {
        let from = Self::extract_file_path(&from)?;
        let to = Self::extract_file_path(&to)?;

        std::fs::copy(&from, &to)
            .with_context(|| format!("Can't transfer from [{}] to [{}].", from.display(), to.display()))?;
        Ok(())
    }

    fn supports(&self, prefix: &str) -> bool {
        prefix.is_empty() || prefix == "file"
    }
}

impl LocalTransfer {
    pub fn new() -> Arc<Box<dyn TransferProtocol>> {
        Arc::new(Box::new(LocalTransfer{}))
    }

    fn extract_file_path(url: &Url) -> Result<PathBuf> {
        Ok(url.to_file_path()
            .map_err(|_| Error::msg(format!("Invalid file path [{}].", url)))?)
    }
}





