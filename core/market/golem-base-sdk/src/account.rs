use alloy::consensus::{
    EthereumTxEnvelope, EthereumTypedTransaction, SignableTransaction, Signed, TxEip4844,
    TxEip4844Variant,
};
use alloy::network::TransactionBuilder;
use alloy::primitives::{address, keccak256, Address, B256, U256};
use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::eth::TransactionRequest;
use alloy::rpc::types::TransactionReceipt;
use alloy::signers::Signature;
use alloy_rlp::{Decodable, Encodable};
use anyhow::anyhow;
use async_trait::async_trait;
use bigdecimal::BigDecimal;
use std::sync::Arc;

use crate::entity::StorageTransaction;
use crate::utils::eth_to_wei;

/// The address of the GolemBase storage processor contract
pub const GOLEM_BASE_STORAGE_PROCESSOR_ADDRESS: Address =
    address!("0x0000000000000000000000000000000060138453");

/// Event signature for entity creation logs
pub fn golem_base_storage_entity_created() -> B256 {
    keccak256(b"GolemBaseStorageEntityCreated(uint256,uint256)")
}

/// Event signature for entity deletion logs
pub fn golem_base_storage_entity_deleted() -> B256 {
    keccak256(b"GolemBaseStorageEntityDeleted(uint256)")
}

/// Event signature for entity update logs
pub fn golem_base_storage_entity_updated() -> B256 {
    keccak256(b"GolemBaseStorageEntityUpdated(uint256,uint256)")
}

/// Event signature for extending TTL of an entity
pub fn golem_base_storage_entity_ttl_extended() -> B256 {
    keccak256(b"GolemBaseStorageEntityTTLExptended(uint256,uint256)")
}

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
    pub async fn sign_transaction(
        &self,
        tx: TransactionRequest,
    ) -> anyhow::Result<Signed<EthereumTypedTransaction<TxEip4844Variant>>> {
        let tx = tx.build_unsigned()?;
        let bytes = tx.encoded_for_signing();

        let signature = self.signer.sign(&bytes).await?;
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

        let signed = self.sign_transaction(tx).await?;
        let encoded = Self::encode_transaction(&signed)?;

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

    /// Transfers ETH from this account to another address
    pub async fn transfer(
        &self,
        to: Address,
        value: BigDecimal,
    ) -> anyhow::Result<TransactionReceipt> {
        let tx = TransactionRequest::default()
            .with_to(to)
            .with_value(eth_to_wei(value)?)
            .with_gas_limit(21_000)
            .with_max_priority_fee_per_gas(1_000_000_000)
            .with_max_fee_per_gas(20_000_000_000);
        self.send_transaction(tx).await
    }

    /// Funds an account by sending ETH
    pub async fn fund_account(&self, value: BigDecimal) -> anyhow::Result<TransactionReceipt> {
        let accounts = self.provider.get_accounts().await?;
        let funder = accounts[0];

        let nonce = self.provider.get_transaction_count(funder).await?;

        let tx = TransactionRequest::default()
            .with_to(self.address())
            .with_from(funder)
            .with_value(eth_to_wei(value)?)
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

    /// Encodes and decodes a transaction for debugging
    fn encode_transaction(
        signed: &Signed<EthereumTypedTransaction<TxEip4844Variant>>,
    ) -> anyhow::Result<Vec<u8>> {
        let mut encoded = Vec::new();
        signed.eip2718_encode(&mut encoded);

        log::debug!(
            "RLP encoded transaction (hash: 0x{:x}): 0x{}",
            signed.hash(),
            hex::encode(&encoded)
        );

        // Decode the transaction for debugging purposes.
        let decoded_tx = EthereumTxEnvelope::<TxEip4844>::decode(&mut &encoded[..])
            .map_err(|e| anyhow!("Failed to decode transaction: {e}"))?;
        let signer = decoded_tx.recover_signer()?;
        log::debug!("Decoded transaction: {:#?}", decoded_tx);
        log::debug!("Recovered signer: {:#?}", signer);

        Ok(encoded)
    }

    /// Gets the account's ETH balance
    pub async fn get_balance(&self) -> anyhow::Result<U256> {
        Ok(self.provider.get_balance(self.address()).await?)
    }
}
