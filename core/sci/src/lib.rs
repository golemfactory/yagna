use ethereum_types::{Address, Secret, H256, U256};
use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::transports::Http;
use web3::types::BlockNumber;

mod eth_client;
pub mod sc_error;
use eth_client::EthClient;

#[allow(unused)]
pub struct SCInterface {
    eth_client: EthClient,
    priv_key: Option<Secret>,
    eth_address: Address,
    gnt_contract: Option<Contract<Http>>,
    gntb_contract: Option<Contract<Http>>,
    gnt_deposit_contract: Option<Contract<Http>>,
    faucet_contract: Option<Contract<Http>>,
}

impl SCInterface {
    pub fn new(transport: Http, ethereum_address: &str) -> SCInterface {
        SCInterface {
            eth_client: EthClient::new(transport),
            priv_key: None,
            eth_address: ethereum_address.parse().unwrap(),
            gnt_contract: None,
            gntb_contract: None,
            gnt_deposit_contract: None,
            faucet_contract: None,
        }
    }

    /// Get Ethereum address
    pub fn get_eth_address(&self) -> Address {
        self.eth_address
    }

    /// Get Ethereum balance
    pub fn get_eth_balance(&self, block_number: Option<BlockNumber>) -> U256 {
        self.eth_client.get_balance(self.eth_address, block_number)
    }

    /// Get Ethereum block number
    pub fn get_block_number(&self) -> U256 {
        self.eth_client.get_block_number()
    }

    /// Get current Gas price
    pub fn get_gas_price(&self) -> U256 {
        self.eth_client.get_gas_price()
    }

    #[allow(unused)]
    pub fn request_gnt_from_faucet(&self) {
        unimplemented!();
    }

    /// Get GNT balance
    pub fn get_gnt_balance(&mut self, address: &str) -> H256 {
        match &self.gnt_contract {
            Some(contract) => {
                let account: Address = address.parse().unwrap();
                let result = contract.call(
                    "balanceOf",
                    (account,),
                    self.eth_address,
                    Options::default(),
                );
                let balance_of: H256 = result.wait().unwrap();
                balance_of
            }
            None => panic!("GNT contract is not bound!"),
        }
    }

    /// Transfer GNT
    #[allow(unused)]
    pub fn transfer_gnt(
        &self,
        amount: U256,
        receiver_address: Address,
        private_key: H256,
        chain_id: u64,
    ) -> H256 {
        unimplemented!();
    }

    /// Get GNTB balance
    pub fn get_gntb_balance(&mut self, address: &str) -> H256 {
        match &self.gntb_contract {
            Some(contract) => {
                let account: Address = address.parse().unwrap();
                let result = contract.call(
                    "balanceOf",
                    (account,),
                    self.eth_address,
                    Options::default(),
                );
                let balance_of: H256 = result.wait().unwrap();
                balance_of
            }
            None => panic!("GNTB contract is not bound!"),
        }
    }

    /// Transfer GNTB
    #[allow(unused)]
    pub fn transfer_gntb(&self, amount: u128, receiver_address: &str) {
        unimplemented!();
    }

    /// Bind GNT contract
    pub fn bind_gnt_contract(&mut self, address: &str) -> &mut Self {
        let contract_address: Address = address.parse().unwrap();
        self.gnt_contract = Some(
            self.eth_client
                .get_contract(contract_address, include_bytes!("./contracts/gnt.json")),
        );
        self
    }

    /// Bind GNTB contract
    pub fn bind_gntb_contract(&mut self, address: &str) -> &mut Self {
        let contract_address: Address = address.parse().unwrap();
        self.gntb_contract = Some(
            self.eth_client
                .get_contract(contract_address, include_bytes!("./contracts/gntb.json")),
        );
        self
    }

    /// Bind GNT deposit contract
    pub fn bind_gnt_deposit_contract(&mut self, address: &str) -> &mut Self {
        let contract_address: Address = address.parse().unwrap();
        self.gnt_deposit_contract = Some(self.eth_client.get_contract(
            contract_address,
            include_bytes!("./contracts/gnt_deposit.json"),
        ));
        self
    }

    /// Bind Faucet contract
    pub fn bind_faucet_contract(&mut self, address: &str) -> &mut Self {
        let contract_address: Address = address.parse().unwrap();
        self.faucet_contract = Some(
            self.eth_client
                .get_contract(contract_address, include_bytes!("./contracts/faucet.json")),
        );
        self
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_it_works() {
        assert_eq!(2 + 3, 5);
    }
}
