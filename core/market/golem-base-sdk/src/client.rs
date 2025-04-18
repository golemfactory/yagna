use alloy::primitives::{Address, B256, U256};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use url::Url;

use crate::account::{Account, TransactionSigner};
use crate::entity::{Create, StorageTransaction};
use crate::signers::{GolemBaseSigner, InMemorySigner};

/// Client for interacting with Golem Base node
#[derive(Clone)]
pub struct GolemBaseClient {
    /// The underlying provider for making RPC calls
    provider: Arc<Box<DynProvider>>,
    /// The chain ID of the connected network
    chain_id: u64,
    accounts: Arc<RwLock<HashMap<Address, Account>>>,
}

impl GolemBaseClient {
    /// Creates a new client
    pub async fn new(endpoint: Url) -> anyhow::Result<Self> {
        let provider = Arc::new(Box::new(ProviderBuilder::new().on_http(endpoint).erased()));
        let chain_id = provider.get_chain_id().await?;

        Ok(Self {
            provider,
            chain_id,
            accounts: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Gets the chain ID of the connected node
    pub fn get_chain_id(&self) -> u64 {
        self.chain_id
    }

    /// Registers a user-managed account with custom signer.
    pub fn account_register(&self, signer: impl TransactionSigner + 'static) -> Address {
        let address = signer.address();
        let mut accounts = self.accounts.write().unwrap();
        accounts.insert(
            address,
            Account {
                signer: Arc::new(Box::new(signer)),
                provider: self.provider.clone(),
                chain_id: self.chain_id,
            },
        );
        address
    }

    /// Generates a new local key, saves it to a keystore file, and registers it
    pub fn account_generate(&self, password: &str) -> anyhow::Result<Address> {
        let signer = InMemorySigner::generate();
        let _path = signer
            .save(password)
            .map_err(|e| anyhow::anyhow!("Failed to save account: {e}"))?;
        Ok(self.account_register(signer))
    }

    /// Loads a key from the default directory and registers it
    pub async fn account_load(&self, address: Address, password: &str) -> anyhow::Result<Address> {
        // This will load all available accounts from GolemBase.
        // We check only the registered accounts, because sync returns local as well.
        let all_accounts = self.account_sync().await?;
        if self.accounts_list().contains(&address) {
            return Ok(address);
        }

        if !all_accounts.contains(&address) {
            return Err(anyhow::anyhow!(
                "Account {address} not found in available accounts"
            ));
        }

        // Try to load from local keystore if it wasn't loaded from GolemBase.
        let signer = InMemorySigner::load_by_address(address, password)?;
        return Ok(self.account_register(signer));
    }

    /// Lists all registered accounts
    pub fn accounts_list(&self) -> Vec<Address> {
        let accounts = self.accounts.read().unwrap();
        accounts.keys().cloned().collect()
    }

    /// Synchronizes accounts with GolemBase, adding any new accounts to our local state
    pub async fn account_sync(&self) -> anyhow::Result<Vec<Address>> {
        let chain_id = self.get_chain_id();

        // Sync GolemBase accounts
        self.sync_golem_base_accounts(chain_id).await?;

        // Get all available accounts
        let mut all_accounts = self.accounts_list();
        let local_accounts = InMemorySigner::list_local_accounts()?;

        // Add local accounts that aren't already in the list
        for address in local_accounts {
            if !all_accounts.contains(&address) {
                all_accounts.push(address);
            }
        }

        Ok(all_accounts)
    }

    /// Funds an account with ETH
    pub async fn fund(&self, account: Address, value: U256) -> anyhow::Result<B256> {
        let account = self.account_get(account)?;
        let receipt = account.fund_account(value).await?;
        Ok(receipt.transaction_hash)
    }

    async fn sync_golem_base_accounts(&self, chain_id: u64) -> anyhow::Result<()> {
        let golem_accounts = self.list_golem_accounts().await?;
        let mut accounts = self.accounts.write().unwrap();

        for address in golem_accounts {
            self.try_insert_account(&mut accounts, address, |address| {
                Box::new(GolemBaseSigner::new(
                    address,
                    self.provider.clone(),
                    chain_id,
                ))
            });
        }

        Ok(())
    }

    fn try_insert_account<F>(
        &self,
        accounts: &mut HashMap<Address, Account>,
        address: Address,
        create_signer: F,
    ) where
        F: FnOnce(Address) -> Box<dyn TransactionSigner>,
    {
        if accounts.contains_key(&address) {
            return;
        }

        let signer = create_signer(address);
        accounts.insert(
            address,
            Account {
                signer: Arc::new(signer),
                provider: self.provider.clone(),
                chain_id: self.chain_id,
            },
        );
    }

    /// Gets an account by its address
    pub fn account_get(&self, address: Address) -> anyhow::Result<Account> {
        let accounts = self.accounts.read().unwrap();
        accounts
            .get(&address)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Account not found"))
    }

    /// Internal function to list accounts from GolemBase
    async fn list_golem_accounts(&self) -> anyhow::Result<Vec<Address>> {
        Ok(self.provider.get_accounts().await?)
    }

    /// Creates an entry using the specified account
    pub async fn create_entry(&self, account: Address, entry: Create) -> anyhow::Result<B256> {
        let account = self.account_get(account)?;
        let tx = StorageTransaction {
            create: vec![entry],
            update: vec![],
            delete: vec![],
            extend: vec![],
        };

        log::debug!("Sending storage transaction from {}", account.address());

        let receipt = account.send_db_transaction(tx).await?;
        Ok(receipt.transaction_hash)
    }

    /// Retrieves an entry's payload from Golem Base by its ID
    pub async fn cat(&self, _id: B256) -> anyhow::Result<Vec<u8>> {
        todo!("Implement cat")
    }

    /// Gets an account's ETH balance
    pub async fn get_balance(&self, account: Address) -> anyhow::Result<U256> {
        Ok(self.provider.get_balance(account).await?)
    }
}
