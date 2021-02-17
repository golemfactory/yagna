use ethereum_types::{Address, H256, U256, U64};
use std::borrow::Cow;

use crate::networks::Network;
use crate::GNTDriverError;
use std::time::Duration;
use web3::contract::Contract;
use web3::transports::Http;
use web3::types::{Bytes, TransactionId, TransactionReceipt};
use web3::Web3;

fn geth_address(network: Network) -> Cow<'static, str> {
    match network {
        Network::Rinkeby => std::env::var("ERC20_RINKEBY_GETH_ADDR")
            .map(Cow::Owned)
            .unwrap_or(Cow::Borrowed("http://geth.testnet.golem.network:55555")),
        Network::Mainnet => std::env::var("ERC20_MAINNET_GETH_ADDR")
            .map(Cow::Owned)
            .unwrap_or(Cow::Borrowed("https://geth.golem.network:55555")),
    }
}

type EthereumClientResult<T> = Result<T, GNTDriverError>;

pub struct EthereumClientBuilder {
    geth_address: Cow<'static, str>,
}

impl EthereumClientBuilder {
    pub fn with_network(network: Network) -> Self {
        let geth_address = geth_address(network);
        Self { geth_address }
    }

    pub fn build(self) -> EthereumClientResult<EthereumClient> {
        let transport = web3::transports::Http::new(self.geth_address.as_ref())?;
        Ok(EthereumClient {
            web3: Web3::new(transport),
        })
    }
}

pub struct EthereumClient {
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
        let gas_price = self.web3.eth().gas_price().await?;
        Ok(gas_price)
    }

    pub async fn send_tx(&self, signed_tx: Vec<u8>) -> EthereumClientResult<H256> {
        let tx_hash = self
            .web3
            .eth()
            .send_raw_transaction(Bytes::from(signed_tx))
            .await?;
        Ok(tx_hash)
    }

    pub async fn blocks(
        &self,
    ) -> EthereumClientResult<impl futures3::stream::Stream<Item = EthereumClientResult<H256>>>
    {
        use futures3::prelude::*;
        let f = self.web3.eth_filter().create_blocks_filter().await?;
        Ok(f.stream(Duration::from_secs(30))
            .map(|v| v.map_err(From::from)))
    }

    pub async fn block_number(&self) -> EthereumClientResult<U64> {
        Ok(self.web3.eth().block_number().await?)
    }

    pub async fn tx_block_number(&self, tx_hash: H256) -> EthereumClientResult<Option<U64>> {
        if let Some(tx) = self
            .web3
            .eth()
            .transaction(TransactionId::Hash(tx_hash))
            .await?
        {
            Ok(tx.block_number)
        } else {
            Ok(None)
        }
    }

    pub async fn get_next_nonce(&self, eth_address: Address) -> EthereumClientResult<U256> {
        let nonce = self.web3.eth().transaction_count(eth_address, None).await?;
        Ok(nonce)
    }

    pub async fn get_transaction_receipt(
        &self,
        tx_hash: H256,
    ) -> EthereumClientResult<Option<TransactionReceipt>> {
        let tx_receipt = self.web3.eth().transaction_receipt(tx_hash).await?;
        Ok(tx_receipt)
    }

    pub async fn get_balance(&self, address: Address) -> EthereumClientResult<U256> {
        let balance = self.web3.eth().balance(address, None).await?;
        Ok(balance)
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
        Ok(EthereumClientBuilder::with_network(Network::Rinkeby).build()?)
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
