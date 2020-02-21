use async_trait::async_trait;

use chrono::{DateTime, Utc};

use ethereum_types::{Address, H256, U256};

use ethereum_tx_sign::RawTransaction;

use web3::contract::tokens::Tokenize;
use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::transports::Http;

use crate::account::{AccountBalance, Balance, Currency};
use crate::error::PaymentDriverError;
use crate::ethereum::EthereumClient;
use crate::payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};
use crate::{PaymentDriver, PaymentDriverResult};

// const GAS_GNT_TRANSFER: u128 = 55000;
const GAS_FAUCET: u128 = 90000;

pub struct GntDriver {
    address: Address,
    ethereum_client: EthereumClient,
    gnt_contract: Contract<Http>,
    faucet_contract: Option<Contract<Http>>,
}

impl GntDriver {
    /// Creates new driver
    pub fn new(
        address: Address,
        ethereum_client: EthereumClient,
        gnt_contract_address: Address,
    ) -> PaymentDriverResult<GntDriver> {
        GntDriver::prepare_contract(
            &ethereum_client,
            gnt_contract_address,
            include_bytes!("./contracts/gnt.json"),
        )
        .map_or_else(
            |e| {
                Err(PaymentDriverError::LibraryError {
                    msg: format!("{:?}", e),
                })
            },
            |contract| {
                Ok(GntDriver {
                    address: address,
                    ethereum_client: ethereum_client,
                    gnt_contract: contract,
                    faucet_contract: None,
                })
            },
        )
    }

    /// Returns Gnt balance
    pub fn get_gnt_balance(
        &self,
        address: ethereum_types::Address,
    ) -> PaymentDriverResult<Balance> {
        self.gnt_contract
            .query("balanceOf", (address,), None, Options::default(), None)
            .wait()
            .map_or_else(
                |e| {
                    Err(PaymentDriverError::LibraryError {
                        msg: format!("{:?}", e),
                    })
                },
                |balance| Ok(Balance::new(balance, Currency::Gnt {})),
            )
    }

    /// Returns ether balance
    pub fn get_eth_balance(&self, address: Address) -> PaymentDriverResult<Balance> {
        let block_number = None;
        self.ethereum_client
            .get_eth_balance(address, block_number)
            .map_or_else(
                |e| {
                    Err(PaymentDriverError::LibraryError {
                        msg: format!("{:?}", e),
                    })
                },
                |amount| Ok(Balance::new(amount, Currency::Eth {})),
            )
    }

    /// Requests Gnt from faucet
    pub async fn request_gnt_from_faucet<F>(&self, tx_sign: F) -> PaymentDriverResult<()>
    where
        F: 'static + FnOnce(Vec<u8>) -> Vec<u8> + Sync + Send,
    {
        match &self.faucet_contract {
            None => Err(PaymentDriverError::LibraryError {
                msg: String::from("Faucet contract not bound"),
            }),
            Some(contract) => {
                let mut tx = self.prepare_raw_tx(U256::from(GAS_FAUCET), contract, "create", ());
                self.send_raw_transaction(&mut tx, tx_sign).map_or_else(
                    |e| {
                        Err(PaymentDriverError::LibraryError {
                            msg: format!("{:?}", e),
                        })
                    },
                    |tx_hash| {
                        println!("Tx hash: {:?}", tx_hash);
                        Ok(())
                    },
                )
            }
        }
    }

    /// Binds faucet contract
    pub fn bind_faucet_contract(
        &mut self,
        faucet_contract_address: Address,
    ) -> PaymentDriverResult<()> {
        GntDriver::prepare_contract(
            &self.ethereum_client,
            faucet_contract_address,
            include_bytes!("./contracts/faucet.json"),
        )
        .map_or_else(
            |e| {
                Err(PaymentDriverError::LibraryError {
                    msg: format!("{:?}", e),
                })
            },
            |contract| {
                self.faucet_contract = Some(contract);
                Ok(())
            },
        )
    }

    fn send_raw_transaction<F>(
        &self,
        raw_tx: &mut RawTransaction,
        sign_tx: F,
    ) -> PaymentDriverResult<H256>
    where
        F: 'static + FnOnce(Vec<u8>) -> Vec<u8> + Sync + Send,
    {
        self.get_gas_price().map_or_else(
            |e| {
                Err(PaymentDriverError::LibraryError {
                    msg: format!("{:?}", e),
                })
            },
            |gas_price| {
                raw_tx.nonce = self.get_next_nonce();
                raw_tx.gas_price = gas_price;
                let chain_id = self.get_chain_id();
                let signature = sign_tx(raw_tx.hash(chain_id));
                let signed_tx = raw_tx.encode_signed_tx(signature, chain_id);
                self.send_transaction(signed_tx).map_or_else(
                    |e| {
                        Err(PaymentDriverError::LibraryError {
                            msg: format!("{:?}", e),
                        })
                    },
                    |tx_hash| Ok(tx_hash),
                )
            },
        )
    }

    fn prepare_raw_tx<P>(
        &self,
        gas: U256,
        contract: &Contract<Http>,
        func: &str,
        tokens: P,
    ) -> RawTransaction
    where
        P: Tokenize,
    {
        RawTransaction {
            // nonce will be overwritten
            nonce: U256::from(0),
            to: Some(contract.address()),
            value: U256::from(0),
            // gas price will be overwritten
            gas_price: U256::from(0),
            gas: gas,
            data: contract.encode(func, tokens).unwrap(),
        }
    }

    fn prepare_contract(
        ethereum_client: &EthereumClient,
        address: Address,
        json_abi: &[u8],
    ) -> PaymentDriverResult<Contract<Http>> {
        ethereum_client.get_contract(address, json_abi).map_or_else(
            |e| {
                Err(PaymentDriverError::LibraryError {
                    msg: format!("{:?}", e),
                })
            },
            |contract| Ok(contract),
        )
    }

    fn get_chain_id(&self) -> u64 {
        self.ethereum_client.get_chain_id()
    }

    fn send_transaction(&self, tx: Vec<u8>) -> PaymentDriverResult<H256> {
        self.ethereum_client.send_tx(tx).map_or_else(
            |e| {
                Err(PaymentDriverError::LibraryError {
                    msg: format!("{:?}", e),
                })
            },
            |tx_hash| Ok(tx_hash),
        )
    }

    fn get_next_nonce(&self) -> U256 {
        let current_nonce = 27_u64;
        U256::from(current_nonce + 1)
    }

    fn get_gas_price(&self) -> PaymentDriverResult<U256> {
        self.ethereum_client.get_gas_price().map_or_else(
            |e| {
                Err(PaymentDriverError::LibraryError {
                    msg: format!("{:?}", e),
                })
            },
            |gas_price| Ok(gas_price),
        )
    }

    fn prepare_payment_amounts(&self, amount: PaymentAmount) -> (U256, U256) {
        let gas_amount = if amount.gas_amount.is_some() {
            amount.gas_amount.unwrap()
        } else {
            U256::from(55000)
        };
        (amount.base_currency_amount, gas_amount)
    }
}

#[async_trait]
impl PaymentDriver for GntDriver {
    /// Returns account balance
    async fn get_account_balance(&self) -> PaymentDriverResult<AccountBalance> {
        self.get_gnt_balance(self.address).map_or_else(
            |e| {
                Err(PaymentDriverError::LibraryError {
                    msg: format!("{:?}", e),
                })
            },
            |gnt_balance| {
                self.get_eth_balance(self.address).map_or_else(
                    |e| {
                        Err(PaymentDriverError::LibraryError {
                            msg: format!("{:?}", e),
                        })
                    },
                    |eth_balance| Ok(AccountBalance::new(gnt_balance, Some(eth_balance))),
                )
            },
        )
    }

    /// Schedules payment
    #[allow(unused)]
    async fn schedule_payment<F>(
        &mut self,
        _invoice_id: &str,
        amount: PaymentAmount,
        recipient: Address,
        due_date: DateTime<Utc>,
        tx_sign: F,
    ) -> PaymentDriverResult<()>
    where
        F: 'static + FnOnce(Vec<u8>) -> Vec<u8> + Sync + Send,
    {
        let (payment_amount, gas_amount) = self.prepare_payment_amounts(amount);
        let mut tx = self.prepare_raw_tx(
            gas_amount,
            &self.gnt_contract,
            "transfer",
            (recipient, payment_amount),
        );
        self.send_raw_transaction(&mut tx, tx_sign).map_or_else(
            |e| {
                Err(PaymentDriverError::LibraryError {
                    msg: format!("{:?}", e),
                })
            },
            |tx_hash| {
                println!("Tx hash: {:?}", tx_hash);
                Ok(())
            },
        )
    }

    /// Returns payment status
    #[allow(unused)]
    async fn get_payment_status(&self, invoice_id: &str) -> PaymentDriverResult<PaymentStatus> {
        unimplemented!();
    }

    /// Verifies payment
    #[allow(unused)]
    async fn verify_payment(
        &self,
        confirmation: &PaymentConfirmation,
    ) -> PaymentDriverResult<PaymentDetails> {
        unimplemented!();
    }

    /// Returns sum of transactions from given address
    #[allow(unused)]
    async fn get_transaction_balance(&self, payee: Address) -> PaymentDriverResult<Balance> {
        unimplemented!();
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use ethereum_types::{Address, U256};

    use web3::transports::Http;

    use super::*;
    use crate::account::{Chain, Currency};

    const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
    const ETH_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";
    const GNT_CONTRACT_ADDRESS: &str = "924442A66cFd812308791872C4B242440c108E19";

    fn to_address(address: &str) -> Address {
        address.parse().unwrap()
    }

    #[test]
    fn test_new_driver() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        let driver = GntDriver::new(
            to_address(ETH_ADDRESS),
            ethereum_client,
            to_address(GNT_CONTRACT_ADDRESS),
        );
        assert!(driver.is_ok());
    }

    #[test]
    fn test_get_eth_balance() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        let driver = GntDriver::new(
            to_address(ETH_ADDRESS),
            ethereum_client,
            to_address(GNT_CONTRACT_ADDRESS),
        )
        .unwrap();
        let eth_balance = driver.get_eth_balance(to_address(ETH_ADDRESS));
        assert!(eth_balance.is_ok());
        let balance = eth_balance.unwrap();
        assert_eq!(balance.currency, Currency::Eth {});
        assert!(balance.amount >= U256::from(0));
    }

    #[test]
    fn test_get_gnt_balance() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        let driver = GntDriver::new(
            to_address(ETH_ADDRESS),
            ethereum_client,
            to_address(GNT_CONTRACT_ADDRESS),
        )
        .unwrap();
        let gnt_balance = driver.get_gnt_balance(to_address(ETH_ADDRESS));
        assert!(gnt_balance.is_ok());
        let balance = gnt_balance.unwrap();
        assert_eq!(balance.currency, Currency::Gnt {});
        assert!(balance.amount >= U256::from(0));
    }

    #[test]
    fn test_get_account_balance() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        let driver = GntDriver::new(
            to_address(ETH_ADDRESS),
            ethereum_client,
            to_address(GNT_CONTRACT_ADDRESS),
        )
        .unwrap();

        let account_balance = block_on(driver.get_account_balance());
        assert!(account_balance.is_ok());
        let balance = account_balance.unwrap();

        let gnt_balance = balance.base_currency;
        assert_eq!(gnt_balance.currency, Currency::Gnt {});
        assert!(gnt_balance.amount >= U256::from(0));

        let some_eth_balance = balance.gas;
        assert!(some_eth_balance.is_some());

        let eth_balance = some_eth_balance.unwrap();
        assert_eq!(eth_balance.currency, Currency::Eth {});
        assert!(eth_balance.amount >= U256::from(0));
    }
}
