use web3::futures::Future;

#[derive(Debug, Clone)]
pub enum EthereumClientError {
    ContractCreationError,
}

type EthereumClientResult<T> = Result<T, EthereumClientError>;

pub struct EthereumClient {
    web3: web3::Web3<web3::transports::Http>,
}

impl EthereumClient {
    pub fn new(transport: web3::transports::Http) -> EthereumClient {
        EthereumClient {
            web3: web3::Web3::new(transport),
        }
    }

    pub fn get_contract(
        &self,
        address: ethereum_types::Address,
        json_abi: &[u8],
    ) -> EthereumClientResult<web3::contract::Contract<web3::transports::Http>> {
        match web3::contract::Contract::from_json(self.web3.eth(), address, json_abi) {
            Ok(c) => Ok(c),
            Err(_) => Err(EthereumClientError::ContractCreationError {}),
        }
    }

    pub fn get_eth_balance(&self, address: ethereum_types::Address, block_number: Option<web3::types::BlockNumber>) -> EthereumClientResult<ethereum_types::U256> {
        // TODO error handling
        Ok(self.web3
            .eth()
            .balance(address, block_number)
            .wait()
            .unwrap())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_it_works() {
        assert!(true);
    }
}
