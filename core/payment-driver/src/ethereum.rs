use ethereum_types::{Address, H256, U256};
use std::env;

use crate::error::PaymentDriverError;
use futures3::compat::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use web3::confirm::{wait_for_confirmations, TransactionReceiptBlockNumberCheck};
use web3::contract::Contract;
use web3::transports::EventLoopHandle;
use web3::transports::Http;
use web3::types::{BlockNumber, Bytes, TransactionReceipt};
use web3::Web3;

const POLL_INTERVAL_SECS: u64 = 1;
const POLL_INTERVAL_NANOS: u32 = 0;

const MAINNET_ID: u64 = 1;
const RINKEBY_ID: u64 = 4;

const CHAIN_ID_ENV_KEY: &str = "CHAIN_ID";
const GETH_ADDRESS_ENV_KEY: &str = "GETH_ADDRESS";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Chain {
    Mainnet,
    Rinkeby,
}

impl Chain {
    pub fn from_env() -> Result<Chain, PaymentDriverError> {
        let chain_id = std::env::var(CHAIN_ID_ENV_KEY)?;
        match chain_id.parse::<u64>().unwrap() {
            MAINNET_ID => Ok(Chain::Mainnet),
            RINKEBY_ID => Ok(Chain::Rinkeby),
            _chain => Err(PaymentDriverError::UnknownChain(_chain)),
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

pub struct EthereumClient {
    chain: Chain,
    _eloop: EventLoopHandle,
    web3: Web3<Http>,
}

impl EthereumClient {
    pub fn new() -> EthereumClientResult<EthereumClient> {
        let (eloop, transport) =
            web3::transports::Http::new(env::var(GETH_ADDRESS_ENV_KEY)?.as_str())?;

        Ok(EthereumClient {
            chain: Chain::from_env()?,
            _eloop: eloop,
            web3: Web3::new(transport),
        })
    }

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

    pub fn get_chain_id(&self) -> u64 {
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
    use std::sync::Once;
    const GNT_CONTRACT_ADDRESS: &str = "0x924442A66cFd812308791872C4B242440c108E19";
    const ETH_ADDRESS: &str = "0x2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";

    static INIT: Once = Once::new();

    fn init_env() {
        INIT.call_once(|| {
            std::env::set_var("GETH_ADDRESS", "http://1.geth.testnet.golem.network:55555");
            std::env::set_var("CHAIN_ID", format!("{:?}", Chain::Rinkeby.id()))
        });
    }

    #[test]
    fn test_get_rinkeby_chain_id() -> anyhow::Result<()> {
        init_env();
        let ethereum_client = EthereumClient::new()?;
        assert_eq!(ethereum_client.get_chain_id(), Chain::Rinkeby.id());
        Ok(())
    }

    #[tokio::test]
    async fn test_get_eth_balance() -> anyhow::Result<()> {
        init_env();
        let ethereum_client = EthereumClient::new()?;
        let address = utils::str_to_addr(ETH_ADDRESS)?;
        let balance: U256 = ethereum_client.get_eth_balance(address, None).await?;
        assert!(balance >= U256::from(0));
        Ok(())
    }

    #[tokio::test]
    async fn test_gas_price() -> anyhow::Result<()> {
        init_env();
        let ethereum_client = EthereumClient::new()?;
        let gas_price: U256 = ethereum_client.get_gas_price().await?;
        assert!(gas_price >= U256::from(0));
        Ok(())
    }

    #[test]
    fn test_get_contract() -> anyhow::Result<()> {
        init_env();
        let ethereum_client = EthereumClient::new()?;
        assert!(ethereum_client
            .get_contract(
                utils::str_to_addr(GNT_CONTRACT_ADDRESS)?,
                include_bytes!("./contracts/gnt.json")
            )
            .is_ok());

        Ok(())
    }

    #[test]
    fn test_get_contract_invalid_abi() -> anyhow::Result<()> {
        init_env();
        let ethereum_client = EthereumClient::new()?;
        assert!(ethereum_client
            .get_contract(utils::str_to_addr(ETH_ADDRESS)?, &[0])
            .is_err());
        Ok(())
    }
}
