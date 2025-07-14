use alloy::network::{AnyNetwork, BlockResponse};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use anyhow::{anyhow, Result};
use golem_base_sdk::client::GolemBaseClient;
use golem_base_sdk::{Address, Hash};

use ya_core_model::market::{
    FundGolemBase, FundGolemBaseResponse, GetGolemBaseBalance, GetGolemBaseBalanceResponse,
    GolemBaseCommand, GolemBaseCommandResponse, GolemBaseCommandType, GolemBaseResponseType,
    RpcMessageError,
};

use crate::config::DiscoveryConfig;
use crate::identity::IdentityApi;
use crate::protocol::discovery::faucet::FaucetClient;

#[derive(Clone)]
pub struct GolemBaseCommandHandler {
    identity: std::sync::Arc<dyn IdentityApi>,
    golem_base: GolemBaseClient,
    config: DiscoveryConfig,
    optimism_client: DynProvider<AnyNetwork>,
}

impl GolemBaseCommandHandler {
    pub fn new(
        identity: std::sync::Arc<dyn IdentityApi>,
        golem_base: GolemBaseClient,
        config: DiscoveryConfig,
    ) -> Self {
        let optimism_provider = Self::create_optimism_provider(&config);

        Self {
            identity,
            golem_base,
            config,
            optimism_client: optimism_provider,
        }
    }

    pub fn from_discovery(discovery: &super::Discovery) -> Self {
        Self::new(
            discovery.inner.identity.clone(),
            discovery.inner.golem_base.clone(),
            discovery.inner.config.clone(),
        )
    }

    fn create_optimism_provider(config: &DiscoveryConfig) -> DynProvider<AnyNetwork> {
        ProviderBuilder::new()
            .network::<AnyNetwork>()
            .connect_http(config.get_rpc_url().clone())
            .erased()
    }

    pub async fn fund(&self, msg: FundGolemBase) -> Result<FundGolemBaseResponse, RpcMessageError> {
        let wallet = match msg.wallet {
            Some(wallet) => wallet,
            None => self.identity.default_identity().await.map_err(|e| {
                RpcMessageError::Market(format!("Failed to get default identity: {e}"))
            })?,
        };

        // Validate account
        let accounts = self
            .identity
            .list()
            .await
            .map_err(|e| RpcMessageError::Market(format!("Failed to list accounts: {}", e)))?;

        let account = accounts
            .iter()
            .find(|acc| acc.node_id == wallet)
            .ok_or_else(|| {
                RpcMessageError::Market(format!("Account {wallet} not found in identities"))
            })?;

        if account.is_locked {
            return Err(RpcMessageError::Market(format!(
                "Account {wallet} is locked"
            )));
        }

        if account.deleted {
            return Err(RpcMessageError::Market(format!(
                "Account {wallet} is deleted"
            )));
        }

        let client = self.golem_base.clone();
        let address = Address::from(&wallet.into_array());

        let faucet_client = FaucetClient::new(self.config.clone(), client.clone());

        if self.config.fund_preallocated() {
            faucet_client
                .fund_local_account(address)
                .await
                .map_err(|e| RpcMessageError::Market(e.to_string()))?;
        } else {
            faucet_client
                .fund_from_faucet_with_pow(&address.to_string())
                .await
                .map_err(|e| RpcMessageError::Market(e.to_string()))?;
        }

        // Get balance after funding
        let balance = client
            .get_balance(address)
            .await
            .map_err(|e| RpcMessageError::Market(format!("Failed to get balance: {}", e)))?;

        log::info!("GolemBase balance for wallet {}: {}", wallet, balance);
        Ok(FundGolemBaseResponse { wallet, balance })
    }

    pub async fn get_balance(
        &self,
        msg: GetGolemBaseBalance,
    ) -> Result<GetGolemBaseBalanceResponse, RpcMessageError> {
        let wallet = match msg.wallet {
            Some(wallet) => wallet,
            None => self.identity.default_identity().await.map_err(|e| {
                RpcMessageError::Market(format!("Failed to get default identity: {e}"))
            })?,
        };

        let client = self.golem_base.clone();
        let address = Address::from(&wallet.into_array());

        let balance = client
            .get_balance(address)
            .await
            .map_err(|e| RpcMessageError::Market(format!("Failed to get balance: {}", e)))?;

        Ok(GetGolemBaseBalanceResponse {
            wallet,
            balance,
            token: "tETH".to_string(),
        })
    }

    pub async fn handle_golem_base_command(
        &self,
        msg: GolemBaseCommand,
    ) -> Result<GolemBaseCommandResponse, RpcMessageError> {
        match msg.command {
            GolemBaseCommandType::GetTransaction { transaction_id } => self
                .get_transaction(transaction_id)
                .await
                .map_err(|e| RpcMessageError::Market(e.to_string())),
            GolemBaseCommandType::GetBlock { block_number } => self
                .get_block(block_number)
                .await
                .map_err(|e| RpcMessageError::Market(e.to_string())),
        }
    }

    async fn get_transaction(
        &self,
        transaction_id: String,
    ) -> anyhow::Result<GolemBaseCommandResponse> {
        let client = self.optimism_client.clone();
        let transaction_hash = transaction_id
            .parse::<Hash>()
            .map_err(|e| anyhow!("Invalid transaction ID format: {}", e))?;

        let transaction = client
            .get_transaction_by_hash(transaction_hash)
            .await
            .map_err(|e| anyhow!("Failed to get transaction: {}", e))?
            .ok_or(anyhow!("Transaction not found: {}", transaction_id))?;

        let transaction_json = serde_json::to_value(&transaction)
            .map_err(|e| anyhow!("Failed to serialize transaction: {}", e))?;

        Ok(GolemBaseCommandResponse {
            response: GolemBaseResponseType::Transaction {
                transaction: transaction_json,
            },
        })
    }

    async fn get_block(&self, block_number: u64) -> anyhow::Result<GolemBaseCommandResponse> {
        let client = self.golem_base.clone();

        // Get block by number
        let block = client
            .get_rpc_client()
            .get_block_by_number(block_number.into())
            .await
            .map_err(|e| anyhow!("Failed to get block: {}", e))?
            .ok_or(anyhow!("Block not found: {}", block_number))?;

        // Get transaction hashes from the block
        let transaction_hashes = block
            .transactions()
            .hashes()
            .map(|h| format!("0x{:x}", h))
            .collect::<Vec<String>>();

        // Manually serialize block data
        let block_json = serde_json::json!({
            "number": block.header.inner.number,
            "parent_hash": format!("0x{:x}", block.header.inner.parent_hash),
            "hash": format!("0x{:x}", block.header.hash),
            "timestamp": block.header.inner.timestamp,
            "nonce": format!("0x{:x}", block.header.inner.nonce),
            "difficulty": block.header.inner.difficulty,
            "total_difficulty": block.header.total_difficulty,
            "size": block.header.size.map(|s| s.to_string()),
            "state_root": format!("0x{:x}", block.header.inner.state_root),
            "receipts_root": format!("0x{:x}", block.header.inner.receipts_root),
            "gas": {
                "limit": block.header.inner.gas_limit,
                "used": block.header.inner.gas_used,
                "base_fee_per_gas": block.header.inner.base_fee_per_gas,
                "blob_gas_used": block.header.inner.blob_gas_used,
                "excess_blob_gas": block.header.inner.excess_blob_gas,
            },
            "withdrawals_root": block.header.inner.withdrawals_root.map(|h| format!("0x{:x}", h)),
            "withdrawals": block.withdrawals.map(|w| w.iter().map(|withdrawal| {
                serde_json::json!({
                    "index": withdrawal.index,
                    "validator_index": withdrawal.validator_index,
                    "address": format!("0x{:x}", withdrawal.address),
                    "amount": format!("0x{:x}", withdrawal.amount),
                })
            }).collect::<Vec<_>>()),
            "transactions_root": format!("0x{:x}", block.header.inner.transactions_root),
            "transaction_hashes": transaction_hashes,
            "logs_bloom": format!("0x{}", hex::encode(block.header.inner.logs_bloom)),
            "extra_data": format!("0x{}", hex::encode(block.header.inner.extra_data)),
        });

        Ok(GolemBaseCommandResponse {
            response: GolemBaseResponseType::Block { block: block_json },
        })
    }
}
