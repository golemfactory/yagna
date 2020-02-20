use crate::transfer_protocol::TransferProtocol;
use std::path::Path;
use anyhow::Error;
use url::Url;


struct LocalTransfer;


impl TransferProtocol for LocalTransfer {
    fn transfer(&self, from: &Url, to: &Url) -> Result<()> {
        unimplemented!();
    }

    fn supports(&self, prefix: &str) -> bool {
        prefix.is_empty()
    }
}





