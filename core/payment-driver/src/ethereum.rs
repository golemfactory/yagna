use ethereum_types::{Address, H256, U256};

use web3::contract::Contract;
use web3::futures::Future;
use web3::transports::Http;
use web3::types::{BlockNumber, Bytes, Filter, FilterBuilder, Log};
use web3::Web3;

use crate::account::Chain;
use crate::error::PaymentDriverError;

type EthereumClientResult<T> = Result<T, PaymentDriverError>;

pub struct EthereumClient {
    web3: Web3<Http>,
    chain: Chain,
}

impl EthereumClient {
    pub fn new(transport: Http, chain: Chain) -> EthereumClient {
        EthereumClient {
            web3: Web3::new(transport),
            chain: chain,
        }
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

    pub fn get_eth_balance(
        &self,
        address: Address,
        block_number: Option<BlockNumber>,
    ) -> EthereumClientResult<U256> {
        self.web3
            .eth()
            .balance(address, block_number)
            .wait()
            .map_or_else(
                |e| Err(PaymentDriverError::LibraryError(format!("{:?}", e))),
                |balance| Ok(balance),
            )
    }

    pub fn get_gas_price(&self) -> EthereumClientResult<U256> {
        self.web3.eth().gas_price().wait().map_or_else(
            |e| Err(PaymentDriverError::LibraryError(format!("{:?}", e))),
            |gas_price| Ok(gas_price),
        )
    }

    pub fn send_tx(&self, signed_tx: Vec<u8>) -> EthereumClientResult<H256> {
        self.web3
            .eth()
            .send_raw_transaction(Bytes::from(signed_tx))
            .wait()
            .map_or_else(
                |e| Err(PaymentDriverError::LibraryError(format!("{:?}", e))),
                |tx_hash| Ok(tx_hash),
            )
    }

    pub fn get_chain_id(&self) -> u64 {
        match self.chain {
            Chain::Mainnet => 1,
            Chain::Rinkeby => 4,
        }
    }

    pub fn get_next_nonce(&self, eth_address: Address) -> EthereumClientResult<U256> {
        self.web3
            .eth()
            .transaction_count(eth_address, None)
            .wait()
            .map_or_else(
                |e| Err(PaymentDriverError::LibraryError(format!("{:?}", e))),
                |nonce| Ok(nonce),
            )
    }

    pub fn get_eth_logs(&self, filter: Filter) -> EthereumClientResult<Vec<Log>> {
        self.web3.eth().logs(filter).wait().map_or_else(
            |e| Err(PaymentDriverError::LibraryError(format!("{:?}", e))),
            |logs| Ok(logs),
        )
    }

    pub fn prepare_filter(&self, address: Address) -> Filter {
        FilterBuilder::default()
            .from_block(BlockNumber::Earliest)
            .to_block(BlockNumber::Latest)
            .address(vec![address])
            .build()
    }
}

#[cfg(test)]
mod tests {
    use ethereum_types::{Address, U256};

    use web3::transports::Http;

    use super::*;

    use crate::account::Chain;

    const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
    const ETH_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";
    const GNT_CONTRACT_ADDRESS: &str = "924442A66cFd812308791872C4B242440c108E19";

    fn to_address(address: &str) -> Address {
        address.parse().unwrap()
    }

    #[test]
    fn test_get_mainnet_chain_id() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Mainnet);
        assert_eq!(ethereum_client.get_chain_id(), 1)
    }

    #[test]
    fn test_get_rinkeby_chain_id() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        assert_eq!(ethereum_client.get_chain_id(), 4)
    }

    #[test]
    fn test_get_eth_balance() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        let address = to_address(ETH_ADDRESS);
        let balance: U256 = ethereum_client.get_eth_balance(address, None).unwrap();
        assert!(balance >= U256::from(0));
    }

    #[test]
    fn test_gas_price() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        let gas_price: U256 = ethereum_client.get_gas_price().unwrap();
        assert!(gas_price >= U256::from(0));
    }

    #[test]
    fn test_get_contract() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        assert!(ethereum_client
            .get_contract(
                to_address(GNT_CONTRACT_ADDRESS),
                include_bytes!("./contracts/gnt.json")
            )
            .is_ok());
    }

    #[test]
    fn test_get_contract_invalid_abi() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        assert!(ethereum_client
            .get_contract(to_address(ETH_ADDRESS), &[0])
            .is_err());
    }
}
