use super::transfer_protocol::TransferProtocol;
use std::path::Path;
use anyhow::{Result, Error };
use url::Url;


struct HttpTransfer;


impl TransferProtocol for HttpTransfer {

    fn transfer(&self, from: &Url, to: &Url) -> Result<()> {
        unimplemented!();
    }

    fn supports(&self, prefix: &str) -> bool {
        match prefix {
            "http" => true,
            "https" => true,
            _ => false
        }
    }
}

