use alloy::consensus::SignableTransaction;
use alloy::network::TransactionBuilder;
use alloy::primitives::{address, Address};
use alloy::rpc::types::eth::TransactionRequest;
use alloy::signers::Signature;
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
    async fn sign(&self, data: &[u8]) -> anyhow::Result<Signature>;
}

/// An account with its signer
#[derive(Clone)]
pub struct Account {
    /// The account's signer
    pub signer: Arc<Box<dyn TransactionSigner>>,
}

impl Account {
    pub fn address(&self) -> Address {
        self.signer.address()
    }

    /// Signs a transaction request
    pub async fn sign(
        &self,
        tx: TransactionRequest,
    ) -> anyhow::Result<
        alloy::consensus::Signed<
            alloy::consensus::EthereumTypedTransaction<alloy::consensus::TxEip4844Variant>,
        >,
    > {
        let tx = tx.build_unsigned()?;
        let hash = tx.signature_hash();

        let signature = self.signer.sign(hash.as_slice()).await?;
        Ok(tx.into_signed(signature))
    }
}
