use anyhow::{Result, Error, Context};
use std::io;
use std::path::Path;
use std::sync::Arc;
use url::Url;

use super::TransferProtocol;


pub struct LocalTransfer;


impl TransferProtocol for LocalTransfer {
    fn transfer(&self, from: &Url, to: &Url) -> Result<()> {
        let from = from.to_file_path()
            .map_err(|_| Error::msg(format!("Invalid source path [{}].", from)))?;
        let to = to.to_file_path()
            .map_err(|_| Error::msg(format!("Invalid source path [{}].", to)))?;

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
}





