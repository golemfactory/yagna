use alloy::consensus::{EthereumTxEnvelope, SignableTransaction, TxEip4844};
use alloy::network::TransactionBuilder;
use alloy::primitives::{address, Address, U256};
use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::eth::TransactionRequest;
use alloy::rpc::types::TransactionReceipt;
use alloy::signers::Signature;
use alloy_rlp::{Decodable, Encodable};
use async_trait::async_trait;
use std::sync::Arc;

use crate::entity::StorageTransaction;

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
    /// The provider for making RPC calls
    pub provider: Arc<Box<DynProvider>>,
    /// The chain ID of the connected network
    pub chain_id: u64,
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

    /// Sends a transaction with common fields filled in
    async fn send_transaction(
        &self,
        mut tx: TransactionRequest,
    ) -> anyhow::Result<TransactionReceipt> {
        let nonce = self.provider.get_transaction_count(self.address()).await?;

        tx = tx
            .with_from(self.address())
            .with_nonce(nonce)
            .with_chain_id(self.chain_id);

        let signed = self.sign(tx).await?;
        let mut encoded = Vec::new();
        signed.eip2718_encode(&mut encoded);

        log::debug!("RLP encoded transaction: 0x{}", hex::encode(&encoded));

        // Decode and display transaction fields
        let decoded_tx = EthereumTxEnvelope::<TxEip4844>::decode(&mut &encoded[..])
            .map_err(|e| anyhow::anyhow!("Failed to decode transaction: {e}"))?;
        log::debug!("Decoded transaction: {:#?}", decoded_tx);

        let pending = self.provider.send_raw_transaction(&encoded).await?;
        Ok(pending.get_receipt().await?)
    }

    /// Creates and sends a storage transaction
    pub async fn send_db_transaction(
        &self,
        tx: StorageTransaction,
    ) -> anyhow::Result<TransactionReceipt> {
        let mut data = Vec::new();
        tx.encode(&mut data);

        let tx = TransactionRequest::default()
            .with_to(GOLEM_BASE_STORAGE_PROCESSOR_ADDRESS)
            .with_gas_limit(1_000_000)
            .with_max_priority_fee_per_gas(1_000_000_000)
            .with_max_fee_per_gas(20_000_000_000)
            .with_input(data.to_vec());

        self.send_transaction(tx).await
    }

    /// Funds an account by sending ETH
    pub async fn fund_account(&self, value: U256) -> anyhow::Result<TransactionReceipt> {
        let accounts = self.provider.get_accounts().await?;
        let funder = accounts[0];

        let nonce = self.provider.get_transaction_count(funder).await?;

        let tx = TransactionRequest::default()
            .with_to(self.address())
            .with_from(funder)
            .with_value(value)
            .with_nonce(nonce)
            .with_chain_id(self.chain_id)
            .with_gas_limit(21_000)
            .with_max_priority_fee_per_gas(1_000_000_000)
            .with_max_fee_per_gas(20_000_000_000);

        Ok(self
            .provider
            .send_transaction(tx)
            .await?
            .get_receipt()
            .await?)
    }

    /// Gets the account's ETH balance
    pub async fn get_balance(&self) -> anyhow::Result<U256> {
        Ok(self.provider.get_balance(self.address()).await?)
    }
}
