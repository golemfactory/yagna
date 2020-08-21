use ethereum_types::{Address, H256, U256};
use futures3::compat::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::env;

use crate::GNTDriverError;
use std::time::Duration;
use web3::contract::Contract;
use web3::transports::EventLoopHandle;
use web3::transports::Http;
use web3::types::{Bytes, TransactionId, TransactionReceipt};
use web3::Web3;

const MAINNET_ID: u64 = 1;
const RINKEBY_ID: u64 = 4;

const MAINNET_NAME: &str = "mainnet";
const RINKEBY_NAME: &str = "rinkeby";

const CHAIN_ENV_VAR: &str = "CHAIN";
const GETH_ADDRESS_ENV_VAR: &str = "GETH_ADDRESS";

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
    pub fn from_env() -> Result<Chain, GNTDriverError> {
        if let Some(chain_name) = env::var(CHAIN_ENV_VAR).ok() {
            match chain_name.as_str() {
                MAINNET_NAME => Ok(Chain::Mainnet),
                RINKEBY_NAME => Ok(Chain::Rinkeby),
                _chain => Err(GNTDriverError::UnknownChain(_chain.into())),
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

type EthereumClientResult<T> = Result<T, GNTDriverError>;

pub struct EthereumClientBuilder {
    geth_address: Cow<'static, str>,
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
        Ok(Self { geth_address })
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
            |e| Err(GNTDriverError::LibraryError(format!("{:?}", e))),
            |contract| Ok(contract),
        )
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

    pub async fn blocks(
        &self,
    ) -> EthereumClientResult<impl futures3::stream::Stream<Item = EthereumClientResult<H256>>>
    {
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

    const NGNT_CONTRACT_ADDRESS: &str = "0xd94e3DC39d4Cad1DAd634e7eb585A57A19dC7EFE";
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
                utils::str_to_addr(NGNT_CONTRACT_ADDRESS)?,
                include_bytes!("../contracts/ngnt.json")
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
