use alloy::network::TransactionBuilder;
use alloy::primitives::Address;
use alloy::providers::DynProvider;
use alloy::providers::Provider;
use alloy::rpc::types::eth::TransactionRequest;
use async_trait::async_trait;
use std::sync::Arc;

use crate::account::TransactionSigner;

/// A signer that uses GolemBase's accounts
pub struct GolemBaseSigner {
    /// The address of the account
    address: Address,
    /// The provider for signing
    provider: Arc<Box<DynProvider>>,
    /// The chain ID for signing
    chain_id: u64,
}

impl GolemBaseSigner {
    /// Creates a new signer for the given address
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

    async fn sign(&self, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        // Get the current nonce
        let nonce = self.provider.get_transaction_count(self.address).await?;

        // Create a transaction from the data
        let tx = TransactionRequest::default()
            .with_from(self.address)
            .with_nonce(nonce)
            .with_chain_id(self.chain_id)
            .with_gas_limit(21_000)
            .with_max_priority_fee_per_gas(1_000_000_000)
            .with_max_fee_per_gas(20_000_000_000)
            .with_input(data.to_vec());

        // Sign the transaction using the provider
        let signed = self.provider.sign_transaction(tx).await?;
        Ok(signed.to_vec())
    }
}
