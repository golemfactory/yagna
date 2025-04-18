use alloy::primitives::Address;
use alloy::providers::DynProvider;

use alloy::signers::Signature;
use async_trait::async_trait;
use std::sync::Arc;

use crate::account::TransactionSigner;

/// A signer that uses GolemBase to sign transactions
pub struct GolemBaseSigner {
    /// The address of the account
    address: Address,
    /// The provider for signing
    provider: Arc<Box<DynProvider>>,
    /// The chain ID for signing
    chain_id: u64,
}

impl GolemBaseSigner {
    /// Creates a new GolemBase signer
    pub fn new(address: Address, provider: Arc<Box<DynProvider>>, chain_id: u64) -> Self {
        Self {
            address,
            provider,
            chain_id,
        }
    }
}

#[async_trait]
impl TransactionSigner for GolemBaseSigner {
    fn address(&self) -> Address {
        self.address
    }

    async fn sign(&self, _data: &[u8]) -> anyhow::Result<Signature> {
        unimplemented!()
    }
}
