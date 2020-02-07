use ethereum_types::{Address, H256, U256};
use web3::contract::Contract;
use web3::futures::Future;
use web3::transports::Http;
use web3::types::{BlockNumber, Bytes};
use web3::Web3;

pub struct EthClient {
    web3: Web3<Http>,
}

impl EthClient {
    pub fn new(transport: Http) -> EthClient {
        EthClient {
            web3: Web3::new(transport),
        }
    }

    pub fn get_contract(&self, address: Address, json_abi: &[u8]) -> Contract<Http> {
        Contract::from_json(self.web3.eth(), address, json_abi).unwrap()
    }

    pub fn get_balance(&self, eth_address: Address, block_number: Option<BlockNumber>) -> U256 {
        self.web3
            .eth()
            .balance(eth_address, block_number)
            .wait()
            .unwrap()
    }

    pub fn get_gas_price(&self) -> U256 {
        self.web3.eth().gas_price().wait().unwrap()
    }

    pub fn send_signed_tx(&self, signed_tx: Vec<u8>) -> H256 {
        self.web3
            .eth()
            .send_raw_transaction(Bytes::from(signed_tx))
            .wait()
            .unwrap()
    }

    pub fn get_block_number(&self) -> U256 {
        self.web3.eth().block_number().wait().unwrap()
    }
}
