use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::transports::Http;
use web3::types::{Address, BlockNumber, U256};
use web3::Web3;

pub struct SCInterface {
    web3: Web3<Http>,
    eth_address: Address,
    gnt_contract: Option<Contract<Http>>,
    gntb_contract: Option<Contract<Http>>,
    gnt_deposit_contract: Option<Contract<Http>>,
    faucet_contract: Option<Contract<Http>>,
}

impl SCInterface {
    pub fn new(http: Http, ethereum_address: &str) -> SCInterface {
        SCInterface {
            web3: Web3::new(http),
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
        self.web3
            .eth()
            .balance(self.eth_address, block_number)
            .wait()
            .unwrap()
    }

    /// Get Ethereum block number
    pub fn get_block_number(&self) -> U256 {
        self.web3.eth().block_number().wait().unwrap()
    }

    /// Get current Gas price
    pub fn get_gas_price(&self) -> U256 {
        self.web3.eth().gas_price().wait().unwrap()
    }

    /// Transfer Ethereum
    // pub fn transfer_eth(&self, amount: u128, receiver_address: &str, gas_price: Option<u128>) {}

    /// Get GNT balance
    pub fn get_gnt_balance(&mut self, address: &str) -> U256 {
        match &self.gnt_contract {
            Some(contract) => {
                let account: Address = address.parse().unwrap();
                let result =
                    contract.query("balanceOf", (account,), None, Options::default(), None);
                let balance_of: U256 = result.wait().unwrap();
                balance_of
            }
            None => panic!("GNT contract is not bound!"),
        }
    }

    /// Transfer GNT
    // pub fn transfer_gnt(&self, amount: u128, receiver_address: &str) {}

    /// Get GNTB balance
    pub fn get_gntb_balance(&mut self, address: &str) -> U256 {
        match &self.gnt_contract {
            Some(contract) => {
                let account: Address = address.parse().unwrap();
                let result =
                    contract.query("balanceOf", (account,), None, Options::default(), None);
                let balance_of: U256 = result.wait().unwrap();
                balance_of
            }
            None => panic!("GNTB contract is not bound!"),
        }
    }

    /// Transfer GNTB
    // pub fn transfer_gntb(&self, amount: u128, receiver_address: &str) {}

    /// Bind GNT contract
    pub fn bind_gnt_contract(&mut self, address: &str) -> &mut Self {
        let contract_address: Address = address.parse().unwrap();
        self.gnt_contract = Some(
            Contract::from_json(
                self.web3.eth(),
                contract_address,
                include_bytes!("./contracts/gnt.json"),
            )
            .unwrap(),
        );
        self
    }

    /// Bind GNTB contract
    pub fn bind_gntb_contract(&mut self, address: &str) -> &mut Self {
        let contract_address: Address = address.parse().unwrap();
        self.gntb_contract = Some(
            Contract::from_json(
                self.web3.eth(),
                contract_address,
                include_bytes!("./contracts/gntb.json"),
            )
            .unwrap(),
        );
        self
    }

    /// Bind GNT deposit contract
    pub fn bind_gnt_deposit_contract(&mut self, address: &str) -> &mut Self {
        let contract_address: Address = address.parse().unwrap();
        self.gnt_deposit_contract = Some(
            Contract::from_json(
                self.web3.eth(),
                contract_address,
                include_bytes!("./contracts/gnt_deposit.json"),
            )
            .unwrap(),
        );
        self
    }

    /// Bind GNT contract
    pub fn bind_faucet_contract(&mut self, address: &str) -> &mut Self {
        let contract_address: Address = address.parse().unwrap();
        self.faucet_contract = Some(
            Contract::from_json(
                self.web3.eth(),
                contract_address,
                include_bytes!("./contracts/faucet.json"),
            )
            .unwrap(),
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
