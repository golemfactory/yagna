use super::transfer_protocol::TransferProtocol;
use std::path::Path;
use anyhow::{Result, Error };
use url::Url;
use std::sync::Arc;


pub struct HttpTransfer;


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

impl HttpTransfer {
    pub fn new() -> Arc<Box<dyn TransferProtocol>> {
        Arc::new(Box::new(HttpTransfer{}))
    }
}


