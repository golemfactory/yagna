use ethereum_tx_sign::RawTransaction;
use ethereum_types::{Address, H256, U256};
use web3::contract::tokens::Tokenize;
use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::transports::Http;
use web3::types::BlockNumber;

mod eth_client;
pub mod sc_error;
use eth_client::EthClient;

pub struct SCInterface {
    eth_client: EthClient,
    chain_id: u64,
    gnt_contract: Option<Contract<Http>>,
    gntb_contract: Option<Contract<Http>>,
    gnt_deposit_contract: Option<Contract<Http>>,
    faucet_contract: Option<Contract<Http>>,
}

impl SCInterface {
    /// Creates new Smart Contract Interface
    pub fn new(transport: Http, chain_id: u64) -> SCInterface {
        SCInterface {
            eth_client: EthClient::new(transport),
            chain_id: chain_id,
            gnt_contract: None,
            gntb_contract: None,
            gnt_deposit_contract: None,
            faucet_contract: None,
        }
    }

    /// Get Ethereum balance
    pub fn get_eth_balance(&self, address: &str, block_number: Option<BlockNumber>) -> U256 {
        self.eth_client
            .get_balance(address.parse().unwrap(), block_number)
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
    /// Request GNT from faucet
    pub fn request_gnt_from_faucet(&self, nonce: U256, private_key: H256) -> H256 {
        match &self.faucet_contract {
            Some(contract) => {
                let tx = self.prepare_raw_tx(nonce, U256::from(90000), contract, "create", ());
                self.sign_and_send_tx(tx, private_key)
            }
            None => panic!("Faucet contract is not bound!"),
        }
    }

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
    pub fn transfer_gnt(
        &self,
        amount: U256,
        receiver_address: &str,
        nonce: U256,
        private_key: H256,
    ) -> H256 {
        match &self.gnt_contract {
            Some(contract) => {
                let address: Address = receiver_address.parse().unwrap();
                let tx = self.prepare_raw_tx(
                    nonce,
                    U256::from(55000),
                    contract,
                    "transfer",
                    (address, amount),
                );
                self.sign_and_send_tx(tx, private_key)
            }
            None => panic!("GNT contract is not bound!"),
        }
    }

    /// Get GNTB balance
    pub fn get_gntb_balance(&mut self, address: &str) -> U256 {
        match &self.gntb_contract {
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
    pub fn transfer_gntb(
        &self,
        amount: U256,
        receiver_address: &str,
        nonce: U256,
        private_key: H256,
    ) -> H256 {
        match &self.gntb_contract {
            Some(contract) => {
                let address: Address = receiver_address.parse().unwrap();
                let tx = self.prepare_raw_tx(
                    nonce,
                    U256::from(55000),
                    contract,
                    "transfer",
                    (address, amount),
                );
                self.sign_and_send_tx(tx, private_key)
            }
            None => panic!("GNTB contract is not bound!"),
        }
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

    /// Send signed transaction
    pub fn send_signed_tx(&self, tx: Vec<u8>) -> H256 {
        self.eth_client.send_signed_tx(tx)
    }

    fn prepare_raw_tx<P>(
        &self,
        nonce: U256,
        gas: U256,
        contract: &Contract<Http>,
        func: &str,
        tokens: P,
    ) -> RawTransaction
    where
        P: Tokenize,
    {
        RawTransaction {
            nonce: nonce,
            to: Some(contract.address()),
            value: U256::from(0),
            gas_price: self.get_gas_price(),
            gas: gas,
            data: contract.encode(func, tokens).unwrap(),
        }
    }

    fn sign_and_send_tx(&self, tx: RawTransaction, private_key: H256) -> H256 {
        let signed_tx = tx.sign(&private_key, &self.chain_id);
        self.send_signed_tx(signed_tx)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_it_works() {
        assert_eq!(2 + 3, 5);
    }
}
