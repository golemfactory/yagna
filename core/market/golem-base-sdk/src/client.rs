use alloy::primitives::{Address, B256};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use serde_json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use url::Url;

use crate::account::{Account, GolemBaseSigner, TransactionSigner};
use crate::entity::Create;

/// Client for interacting with Golem Base node
#[derive(Clone)]
pub struct GolemBaseClient {
    /// The underlying provider for making RPC calls
    provider: Arc<Box<DynProvider>>,
    accounts: Arc<RwLock<HashMap<Address, Account>>>,
}

impl GolemBaseClient {
    /// Creates a new client
    pub fn new(endpoint: Url) -> Self {
        Self {
            provider: Arc::new(Box::new(ProviderBuilder::new().on_http(endpoint).erased())),
            accounts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Gets the chain ID of the connected node
    pub async fn get_chain_id(&self) -> anyhow::Result<u64> {
        Ok(self.provider.get_chain_id().await?)
    }

    /// Registers a user-managed account with its signer
    pub fn register_account(&self, signer: impl TransactionSigner + 'static) -> Address {
        let address = signer.address();
        let mut accounts = self.accounts.write().unwrap();
        accounts.insert(
            address,
            Account {
                address,
                signer: Arc::new(Box::new(signer)),
            },
        );
        address
    }

    /// Lists all registered accounts
    pub fn list_accounts(&self) -> Vec<Address> {
        let accounts = self.accounts.read().unwrap();
        accounts.keys().cloned().collect()
    }

    /// Synchronizes accounts with GolemBase, adding any new accounts to our local state
    pub async fn sync_accounts(&self) -> anyhow::Result<Vec<Address>> {
        // Get accounts from GolemBase
        let golem_accounts = self.list_golem_accounts().await?;

        // Get current local accounts
        let mut local_accounts = self.accounts.write().unwrap();

        // Add any new accounts from GolemBase
        for address in &golem_accounts {
            if !local_accounts.contains_key(address) {
                local_accounts.insert(
                    *address,
                    Account {
                        address: *address,
                        signer: Arc::new(Box::new(GolemBaseSigner::new(*address))),
                    },
                );
            }
        }

        Ok(golem_accounts)
    }

    /// Internal function to list accounts from GolemBase
    async fn list_golem_accounts(&self) -> anyhow::Result<Vec<Address>> {
        Ok(self.provider.get_accounts().await?)
    }

    /// Creates an entry using the specified account
    pub async fn create_entry(&self, account: Address, entry: Create) -> anyhow::Result<B256> {
        // Get the signer reference and release the lock immediately
        let signer = {
            let accounts = self.accounts.read().unwrap();
            let account = accounts
                .get(&account)
                .ok_or_else(|| anyhow::anyhow!("Account not found"))?;
            account.signer.clone()
        };

        // Serialize the entry
        let data = serde_json::to_vec(&entry)?;

        // Sign the data (no lock held during this async operation)
        let signature = signer.sign(&data).await?;

        todo!("Implement create_entry with signing")
    }

    /// Retrieves an entry's payload from Golem Base by its ID
    pub async fn cat(&self, id: B256) -> anyhow::Result<Vec<u8>> {
        todo!("Implement cat")
    }
}
