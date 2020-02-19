use chrono::{DateTime, Utc};

use ethereum_types::Address;

use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::transports::Http;

use crate::account::{AccountBalance, Balance, Currency};
use crate::error::PaymentDriverError;
use crate::ethereum::EthereumClient;
use crate::payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};
use crate::{PaymentDriver, PaymentDriverResult};

pub struct GntDriver {
    address: Address,
    ethereum_client: EthereumClient,
    gnt_contract: Contract<Http>,
}

impl GntDriver {
    pub fn new(
        address: Address,
        ethereum_client: EthereumClient,
        contract_address: Address,
    ) -> PaymentDriverResult<GntDriver> {
        ethereum_client
            .get_contract(contract_address, include_bytes!("./contracts/gnt.json"))
            .map_or_else(
                |e| {
                    Err(PaymentDriverError::LibraryError {
                        msg: format!("{}", e),
                    })
                },
                |contract| {
                    Ok(GntDriver {
                        address: address,
                        ethereum_client: ethereum_client,
                        gnt_contract: contract,
                    })
                },
            )
    }

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
                        msg: format!("{}", e),
                    })
                },
                |balance| Ok(Balance::new(balance, Currency::Gnt {})),
            )
    }

    pub fn get_eth_balance(&self, address: Address) -> PaymentDriverResult<Balance> {
        let block_number = None;
        self.ethereum_client
            .get_eth_balance(address, block_number)
            .map_or_else(
                |e| {
                    Err(PaymentDriverError::LibraryError {
                        msg: format!("{}", e),
                    })
                },
                |amount| Ok(Balance::new(amount, Currency::Eth {})),
            )
    }
}

#[async_trait::async_trait]
impl PaymentDriver for GntDriver {
    /// Returns account balance
    async fn get_account_balance(&self) -> PaymentDriverResult<AccountBalance> {
        self.get_gnt_balance(self.address).map_or_else(
            |e| {
                Err(PaymentDriverError::LibraryError {
                    msg: format!("{}", e),
                })
            },
            |gnt_balance| {
                self.get_eth_balance(self.address).map_or_else(
                    |e| {
                        Err(PaymentDriverError::LibraryError {
                            msg: format!("{}", e),
                        })
                    },
                    |eth_balance| Ok(AccountBalance::new(gnt_balance, Some(eth_balance))),
                )
            },
        )
    }

    /// Schedules payment
    #[allow(unused)]
    async fn schedule_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        recipient: Address,
        due_date: DateTime<Utc>,
    ) -> PaymentDriverResult<()> {
        unimplemented!();
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
    use crate::account::Currency;

    const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
    const ETH_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";
    const GNT_CONTRACT_ADDRESS: &str = "924442A66cFd812308791872C4B242440c108E19";

    fn to_address(address: &str) -> Address {
        address.parse().unwrap()
    }

    #[test]
    fn test_new_driver() {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport);
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
        let ethereum_client = EthereumClient::new(transport);
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
        let ethereum_client = EthereumClient::new(transport);
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
        let ethereum_client = EthereumClient::new(transport);
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
