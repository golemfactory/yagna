#![allow(unused)]
use crate::error::PaymentDriverError;
use ethereum_types::{Address, H256, U256};
use futures3::compat::*;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::env;

use std::time::Duration;
use web3::confirm::{wait_for_confirmations, TransactionReceiptBlockNumberCheck};
use web3::contract::Contract;
use web3::transports::EventLoopHandle;
use web3::transports::Http;
use web3::types::{BlockNumber, Bytes, TransactionId, TransactionReceipt};
use web3::Web3;

const POLL_INTERVAL_SECS: u64 = 1;
const POLL_INTERVAL_NANOS: u32 = 0;

const MAINNET_ID: u64 = 1;
const RINKEBY_ID: u64 = 4;

const MAINNET_NAME: &str = "mainnet";
const RINKEBY_NAME: &str = "rinkeby";

const CHAIN_ENV_VAR: &str = "CHAIN";
const GETH_ADDRESS_ENV_VAR: &str = "GETH_ADDRESS";

lazy_static! {
    pub static ref CHAIN: String = env::var(CHAIN_ENV_VAR)
        .expect(format!("Missing {} env variable...", CHAIN_ENV_VAR).as_str())
        .to_ascii_lowercase();
    pub static ref GETH_ADDRESS: String = env::var(GETH_ADDRESS_ENV_VAR)
        .expect(format!("Missing {} env variable...", GETH_ADDRESS_ENV_VAR).as_str());
}

fn default_geth_address(chain: Chain) -> &'static str {
    match chain {
        Chain::Rinkeby => "http://1.geth.testnet.golem.network:55555",
        Chain::Mainnet => "https://geth.golem.network:55555",
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Chain {
    Mainnet,
    Rinkeby,
}

impl Default for Chain {
    fn default() -> Self {
        Chain::Rinkeby
    }
}

impl Chain {
    pub fn from_env() -> Result<Chain, PaymentDriverError> {
        if let Some(chain_name) = env::var(CHAIN_ENV_VAR).ok() {
            match chain_name.as_str() {
                MAINNET_NAME => Ok(Chain::Mainnet),
                RINKEBY_NAME => Ok(Chain::Rinkeby),
                _chain => Err(PaymentDriverError::UnknownChain(_chain.into())),
            }
        } else {
            Ok(Default::default())
        }
    }

    pub fn id(&self) -> u64 {
        match &self {
            Chain::Mainnet => MAINNET_ID,
            Chain::Rinkeby => RINKEBY_ID,
        }
    }
}

type EthereumClientResult<T> = Result<T, PaymentDriverError>;

pub struct EthereumClientBuilder {
    geth_address: Cow<'static, str>,
    chain: Chain,
}

impl EthereumClientBuilder {
    pub fn from_env() -> EthereumClientResult<Self> {
        let chain = Chain::from_env()?;
        Self::with_chain(chain)
    }

    pub fn with_chain(chain: Chain) -> EthereumClientResult<Self> {
        let geth_address = env::var(GETH_ADDRESS_ENV_VAR)
            .ok()
            .map(Cow::Owned)
            .unwrap_or_else(|| Cow::Borrowed(default_geth_address(chain)));
        Ok(Self {
            chain,
            geth_address,
        })
    }

    #[inline]
    pub fn with_geth_address(mut self, geth_address: Cow<'static, str>) -> Self {
        self.geth_address = geth_address;
        self
    }

    pub fn build(self) -> EthereumClientResult<EthereumClient> {
        let (eloop, transport) = web3::transports::Http::new(self.geth_address.as_ref())?;
        Ok(EthereumClient {
            chain: Chain::from_env()?,
            _eloop: eloop,
            web3: Web3::new(transport),
        })
    }
}

pub struct EthereumClient {
    chain: Chain,
    _eloop: EventLoopHandle,
    web3: Web3<Http>,
}

impl EthereumClient {
    pub fn get_contract(
        &self,
        address: Address,
        json_abi: &[u8],
    ) -> EthereumClientResult<Contract<Http>> {
        Contract::from_json(self.web3.eth(), address, json_abi).map_or_else(
            |e| Err(PaymentDriverError::LibraryError(format!("{:?}", e))),
            |contract| Ok(contract),
        )
    }

    pub async fn get_eth_balance(
        &self,
        address: Address,
        block_number: Option<BlockNumber>,
    ) -> EthereumClientResult<U256> {
        let balance = self
            .web3
            .eth()
            .balance(address, block_number)
            .compat()
            .await?;
        Ok(balance)
    }

    pub async fn get_gas_price(&self) -> EthereumClientResult<U256> {
        let gas_price = self.web3.eth().gas_price().compat().await?;
        Ok(gas_price)
    }

    pub async fn send_tx(&self, signed_tx: Vec<u8>) -> EthereumClientResult<H256> {
        let tx_hash = self
            .web3
            .eth()
            .send_raw_transaction(Bytes::from(signed_tx))
            .compat()
            .await?;
        Ok(tx_hash)
    }

    pub async fn wait_for_confirmations(
        &self,
        tx_hash: H256,
        confirmations: usize,
    ) -> EthereumClientResult<()> {
        let eth_filter = self.web3.eth_filter();
        let check = TransactionReceiptBlockNumberCheck::new(self.web3.eth(), tx_hash);
        wait_for_confirmations(
            self.web3.eth(),
            eth_filter,
            Duration::new(POLL_INTERVAL_SECS, POLL_INTERVAL_NANOS),
            confirmations,
            check,
        )
        .compat()
        .await?;
        Ok(())
    }

    pub async fn blocks(
        &self,
    ) -> EthereumClientResult<impl futures3::stream::Stream<Item = EthereumClientResult<H256>>>
    {
        use futures3::compat::Stream01CompatExt;
        use futures3::prelude::*;
        let f = self
            .web3
            .eth_filter()
            .create_blocks_filter()
            .compat()
            .await?;
        Ok(f.stream(Duration::from_secs(30))
            .compat()
            .map(|v| v.map_err(From::from)))
    }

    pub async fn block_number(&self) -> EthereumClientResult<U256> {
        Ok(self.web3.eth().block_number().compat().await?)
    }

    pub async fn tx_block_number(&self, tx_hash: H256) -> EthereumClientResult<Option<U256>> {
        if let Some(tx) = self
            .web3
            .eth()
            .transaction(TransactionId::Hash(tx_hash))
            .compat()
            .await?
        {
            Ok(tx.block_number)
        } else {
            Ok(None)
        }
    }

    pub fn chain_id(&self) -> u64 {
        self.chain.id()
    }

    pub async fn get_next_nonce(&self, eth_address: Address) -> EthereumClientResult<U256> {
        let nonce = self
            .web3
            .eth()
            .transaction_count(eth_address, None)
            .compat()
            .await?;
        Ok(nonce)
    }

    pub async fn get_transaction_receipt(
        &self,
        tx_hash: H256,
    ) -> EthereumClientResult<Option<TransactionReceipt>> {
        let tx_receipt = self
            .web3
            .eth()
            .transaction_receipt(tx_hash)
            .compat()
            .await?;
        Ok(tx_receipt)
    }
}

#[cfg(test)]
mod tests {
    use ethereum_types::U256;

    use super::*;
    use crate::utils;

    const GNT2_CONTRACT_ADDRESS: &str = "0xd94e3DC39d4Cad1DAd634e7eb585A57A19dC7EFE";
    const ETH_ADDRESS: &str = "0x2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";

    fn eth_client() -> anyhow::Result<EthereumClient> {
        Ok(EthereumClientBuilder::with_chain(Chain::Rinkeby)?.build()?)
    }

    #[test]
    fn test_get_rinkeby_chain_id() -> anyhow::Result<()> {
        let ethereum_client = eth_client()?;
        assert_eq!(ethereum_client.chain_id(), Chain::Rinkeby.id());
        Ok(())
    }

    #[tokio::test]
    async fn test_get_eth_balance() -> anyhow::Result<()> {
        let ethereum_client = eth_client()?;
        let address = utils::str_to_addr(ETH_ADDRESS)?;
        let balance: U256 = ethereum_client.get_eth_balance(address, None).await?;
        assert!(balance >= U256::from(0));
        Ok(())
    }

    #[tokio::test]
    async fn test_gas_price() -> anyhow::Result<()> {
        let ethereum_client = eth_client()?;
        let gas_price: U256 = ethereum_client.get_gas_price().await?;
        assert!(gas_price >= U256::from(0));
        Ok(())
    }

    #[test]
    fn test_get_contract() -> anyhow::Result<()> {
        let ethereum_client = eth_client()?;
        assert!(ethereum_client
            .get_contract(
                utils::str_to_addr(GNT2_CONTRACT_ADDRESS)?,
                include_bytes!("./contracts/gnt2.json")
            )
            .is_ok());

        Ok(())
    }

    #[test]
    fn test_get_contract_invalid_abi() -> anyhow::Result<()> {
        let ethereum_client = eth_client()?;
        assert!(ethereum_client
            .get_contract(utils::str_to_addr(ETH_ADDRESS)?, &[0])
            .is_err());
        Ok(())
    }
}
