use ethereum_types::{Address, H256, U256};

use futures3::compat::*;
use web3::contract::Contract;
use web3::transports::EventLoopHandle;
use web3::transports::Http;
use web3::types::{BlockNumber, Bytes, TransactionReceipt};
use web3::Web3;

use crate::account::Chain;
use crate::error::PaymentDriverError;
use std::time::Duration;

const REQUIRED_CONFIRMATIONS: usize = 5;
const POLL_INTERVAL_SECS: u64 = 1;
const POLL_INTERVAL_NANOS: u32 = 0;

type EthereumClientResult<T> = Result<T, PaymentDriverError>;

pub struct EthereumClient {
    chain: Chain,
    _eloop: EventLoopHandle,
    web3: Web3<Http>,
}

impl EthereumClient {
    pub fn new(chain: Chain, geth_address: &str) -> EthereumClientResult<EthereumClient> {
        let (eloop, transport) = web3::transports::Http::new(geth_address)?;

        Ok(EthereumClient {
            chain: chain,
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
        let confirmation = web3::confirm::send_raw_transaction_with_confirmation(
            &self.web3.transport(),
            Bytes::from(signed_tx),
            Duration::new(POLL_INTERVAL_SECS, POLL_INTERVAL_NANOS),
            REQUIRED_CONFIRMATIONS,
        )
        .compat()
        .await?;

        Ok(confirmation.transaction_hash)
    }

    pub fn get_chain_id(&self) -> u64 {
        match self.chain {
            Chain::Mainnet => 1,
            Chain::Rinkeby => 4,
        }
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
    use ethereum_types::{Address, U256};

    use super::*;

    use crate::account::Chain;

    const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
    const ETH_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";
    const GNT_CONTRACT_ADDRESS: &str = "924442A66cFd812308791872C4B242440c108E19";

    fn to_address(address: &str) -> Address {
        address.parse().unwrap()
    }

    #[test]
    fn test_get_mainnet_chain_id() -> anyhow::Result<()> {
        let ethereum_client = EthereumClient::new(Chain::Mainnet, GETH_ADDRESS)?;
        assert_eq!(ethereum_client.get_chain_id(), 1);
        Ok(())
    }

    #[test]
    fn test_get_rinkeby_chain_id() -> anyhow::Result<()> {
        let ethereum_client = EthereumClient::new(Chain::Rinkeby, GETH_ADDRESS)?;
        assert_eq!(ethereum_client.get_chain_id(), 4);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_eth_balance() -> anyhow::Result<()> {
        let ethereum_client = EthereumClient::new(Chain::Rinkeby, GETH_ADDRESS)?;
        let address = to_address(ETH_ADDRESS);
        let balance: U256 = ethereum_client.get_eth_balance(address, None).await?;
        assert!(balance >= U256::from(0));
        Ok(())
    }

    #[tokio::test]
    async fn test_gas_price() -> anyhow::Result<()> {
        let ethereum_client = EthereumClient::new(Chain::Rinkeby, GETH_ADDRESS)?;
        let gas_price: U256 = ethereum_client.get_gas_price().await?;
        assert!(gas_price >= U256::from(0));
        Ok(())
    }

    #[test]
    fn test_get_contract() -> anyhow::Result<()> {
        let ethereum_client = EthereumClient::new(Chain::Rinkeby, GETH_ADDRESS)?;
        assert!(ethereum_client
            .get_contract(
                to_address(GNT_CONTRACT_ADDRESS),
                include_bytes!("./contracts/gnt.json")
            )
            .is_ok());

        Ok(())
    }

    #[test]
    fn test_get_contract_invalid_abi() -> anyhow::Result<()> {
        let ethereum_client = EthereumClient::new(Chain::Rinkeby, GETH_ADDRESS)?;
        assert!(ethereum_client
            .get_contract(to_address(ETH_ADDRESS), &[0])
            .is_err());
        Ok(())
    }
}
