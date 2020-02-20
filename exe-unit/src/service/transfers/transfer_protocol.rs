use anyhow::Result;
use url::Url;


/// Implements protocol for transfering files between remote
/// and local locations.
pub trait TransferProtocol {

    fn transfer(&self, from: &Url, to: &Url) -> Result<()>;
    fn supports(&self, prefix: &str) -> bool;
}

