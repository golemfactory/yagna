use ethereum_types::{Address, U256};

use web3::contract::Contract;
use web3::futures::Future;
use web3::transports::Http;
use web3::types::BlockNumber;
use web3::Web3;

use crate::error::PaymentDriverError;

type EthereumClientResult<T> = Result<T, PaymentDriverError>;

pub struct EthereumClient {
    web3: Web3<Http>,
}

impl EthereumClient {
    pub fn new(transport: Http) -> EthereumClient {
        EthereumClient {
            web3: Web3::new(transport),
        }
    }

    pub fn get_contract(
        &self,
        address: Address,
        json_abi: &[u8],
    ) -> EthereumClientResult<Contract<Http>> {
        Contract::from_json(self.web3.eth(), address, json_abi).map_or_else(
            |e| {
                Err(PaymentDriverError::LibraryError {
                    msg: format!("{}", e),
                })
            },
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
                |e| {
                    Err(PaymentDriverError::LibraryError {
                        msg: format!("{}", e),
                    })
                },
                |balance| Ok(balance),
            )
    }
}

#[cfg(test)]
mod tests {
    use ethereum_types::{Address, U256};

    use web3::transports::Http;

    use super::*;

    const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
    const ETH_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";
    const GNT_CONTRACT_ADDRESS: &str = "924442A66cFd812308791872C4B242440c108E19";

    fn to_address(address: &str) -> Address {
        address.parse().unwrap()
    }

    #[test]
    fn test_get_eth_balance() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport);
        let address = to_address(ETH_ADDRESS);
        let balance: U256 = ethereum_client.get_eth_balance(address, None).unwrap();
        assert!(balance >= U256::from(0));
    }

    #[test]
    fn test_get_contract() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport);
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
        let ethereum_client = EthereumClient::new(transport);
        assert!(ethereum_client
            .get_contract(to_address(ETH_ADDRESS), &[0])
            .is_err());
    }
}
