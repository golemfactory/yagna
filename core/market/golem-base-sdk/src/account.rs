use alloy::primitives::Address;
use async_trait::async_trait;
use std::sync::Arc;

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
    /// The account's address
    pub address: Address,
    /// The account's signer
    pub signer: Arc<Box<dyn TransactionSigner>>,
}

/// A signer that uses GolemBase's accounts
pub struct GolemBaseSigner {
    /// The address of the account
    address: Address,
}

impl GolemBaseSigner {
    /// Creates a new signer for the given address
    pub fn new(address: Address) -> Self {
        Self { address }
    }
}

#[async_trait]
impl TransactionSigner for GolemBaseSigner {
    fn address(&self) -> Address {
        self.address
    }

    async fn sign(&self, _data: &[u8]) -> anyhow::Result<Vec<u8>> {
        todo!("Implement signing with GolemBase")
    }
}
