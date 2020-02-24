use super::transfer_protocol::TransferProtocol;
use super::cache::ContentHash;

use anyhow::{Result, Error, Context};
use lazy_static::lazy_static;
use regex::Regex;
use std::sync::Arc;
use std::path::{Path, PathBuf};
use url::Url;




pub struct Transfers {
    protocols: Vec<Arc<Box<dyn TransferProtocol>>>,
}

impl Transfers {

    pub fn new() -> Transfers {
        Transfers{protocols: vec![]}
    }

    pub fn transfer(&self, from: &Url, to: &Url, local_root: &Path) -> Result<Url> {
        let src_url = Self::translate_local_path(from, local_root)?;
        let dest_url = Self::translate_local_path(to, local_root)?;

        let protocol = self.find_protocol(&src_url, &dest_url)?;
        protocol.transfer(&src_url, &dest_url)?;
        Ok(dest_url)
    }

    pub fn register_protocol(&mut self, protocol: Arc<Box<dyn TransferProtocol>>) {
        self.protocols.push(protocol);
    }

    fn find_protocol(&self, from: &Url, to: &Url) -> Result<Arc<Box<dyn TransferProtocol>>> {
        let src_prefix = from.scheme();
        let dest_prefix = to.scheme();

        if !Self::is_local(dest_prefix) && !Self::is_local(src_prefix) {
            return Err(Error::msg(format!("One of urls source [{}] or destination [{}] must be local path.", from, to)));
        };

        // Choose prefix of remote path. If both paths are local paths
        // remote_prefix will be empty and we will do local copy of file.
        let remote_prefix = if !Self::is_local(src_prefix) {
            src_prefix
        } else {
            dest_prefix
        };

        if let Some(protocol) = self.protocols.iter().find(|protocol| protocol.supports(remote_prefix)) {
            Ok(protocol.clone())
        }
        else {
            Err(Error::msg(format!("Protocol for transfering from [{}] to [{}] not found.", from, to)))
        }
    }

    fn is_local(prefix: &str) -> bool {
        prefix.is_empty() || prefix == "file"
    }

    /// Translates all local paths to be relative to workdir.
    /// We do this, because remote requestor shouldn't know our file system.
    /// We treat working directory as filesystem root.
    pub fn translate_local_path(path: &Url, root: &Path) -> Result<Url> {
        let prefix = path.scheme();
        if Self::is_local(&prefix) {
            // Remove root from path, which we know is absolute.
            let original_without_prefix = PathBuf::from(path.path());
            let relative = original_without_prefix.strip_prefix("/")?;
            let absolute = root.join(relative);
            Ok(Url::parse(&format!("file://{}", absolute.display()))?)
        }
        else {
            // Don't translate remote path.
            Ok(path.clone())
        }
    }

    pub fn extract_hash(url: &str) -> Result<(Option<ContentHash>, Url)> {
//        lazy_static! {
//            static ref hash_regex: Regex = Regex::new("^hash://([[:alnum:]]+):([[:digit:]]+):([[:print:]]+)$").unwrap();
//        }
//
//        match hash_regex.captures(url) {
//            Ok(captures) => {
//                let algorithm = captures[1];
//                let digest = captures[2];
//                let url = captures[3];
//
//                return Ok((Some(ContentHash{algorithm, digest}), url.clone()));
//            },
//            Err(error) => {
//                let url = Url::parse(url)?;
//                return Ok((None, url));
//            }
//        }
        unimplemented!();
    }

    pub fn validate_hash(url: &Url, hash: &ContentHash) -> Result<()> {
        unimplemented!();
    }
}


