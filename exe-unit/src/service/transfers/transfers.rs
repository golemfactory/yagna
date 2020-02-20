use super::transfer_protocol::TransferProtocol;

use anyhow::{Result, Error, Context};
use url::Url;
use std::sync::Arc;


pub struct Transfers {
    protocols: Vec<Arc<Box<dyn TransferProtocol>>>,
}

impl Transfers {

    pub fn new() -> Transfers {
        Transfers{protocols: vec![]}
    }

    pub fn transfer(&self, from: &str, to: &str) -> Result<()> {
        let src_url = Url::parse(from)
            .with_context(|| format!("Can't parse source URL [{}].", from))?;
        let dest_url = Url::parse(from)
            .with_context(|| format!("Can't parse destination URL [{}].", from))?;

        let protocol = self.find_protocol(&src_url, &dest_url)?;
        Ok(protocol.transfer(&src_url, &dest_url)?)
    }

    pub fn register_protocol(&mut self, protocol: Arc<Box<dyn TransferProtocol>>) {
        self.protocols.push(protocol);
    }

    fn find_protocol(&self, from: &Url, to: &Url) -> Result<Arc<Box<dyn TransferProtocol>>> {
        let src_prefix = from.scheme();
        let dest_prefix = to.scheme();

        if !dest_prefix.is_empty() && !src_prefix.is_empty() {
            return Err(Error::msg(format!("One of urls source [{}] or destination [{}] must be local path.", from, to)));
        };

        // Choose prefix of remote path. If both paths are local paths
        // remote_prefix will be empty and we will do local copy of file.
        let remote_prefix = if !src_prefix.is_empty() {
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
}


