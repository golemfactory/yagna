use alloy::primitives::{address, Address};
use async_trait::async_trait;
use std::sync::Arc;

/// The address of the GolemBase storage processor contract
pub const GOLEM_BASE_STORAGE_PROCESSOR_ADDRESS: Address =
    address!("0x0000000000000000000000000000000060138453");

/// A trait for signing transactions
#[async_trait]
pub trait TransactionSigner: Send + Sync {
    /// Returns the address of the signer
    fn address(&self) -> Address;

    /// Signs the given data
    async fn sign(&self, data: &[u8]) -> anyhow::Result<Vec<u8>>;
}

/// An account with its signer
pub struct Account {
    /// The account's signer
    pub signer: Arc<Box<dyn TransactionSigner>>,
}

impl Account {
    pub fn address(&self) -> Address {
        self.signer.address()
    }
}
