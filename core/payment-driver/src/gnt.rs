use async_trait::async_trait;

use chrono::{DateTime, Utc};

use ethereum_types::{Address, H256, U256};

use ethereum_tx_sign::RawTransaction;

use std::collections::HashMap;

use std::{thread, time};

use web3::contract::tokens::Tokenize;
use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::transports::Http;

use ya_persistence::executor::DbExecutor;

use crate::account::{AccountBalance, Balance, Currency};
use crate::dao::payment::PaymentDao;
use crate::dao::transaction::TransactionDao;
use crate::error::{DbResult, PaymentDriverError};
use crate::ethereum::EthereumClient;
use crate::models::{PaymentEntity, TransactionEntity};
use crate::payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};
use crate::{PaymentDriver, PaymentDriverResult, SignTx};

const GNT_TRANSFER_GAS: u32 = 55000;
const GNT_FAUCET_GAS: u32 = 90000;

const MAX_ETH_FAUCET_REQUESTS: u32 = 10;
const ETH_FAUCET_SLEEP_SECONDS: u64 = 1;

const MAX_TESTNET_BALANCE: &str = "10000000000000";

pub struct GntDriver {
    ethereum_client: EthereumClient,
    gnt_contract: Contract<Http>,
    db: DbExecutor,
}

impl GntDriver {
    /// Creates new driver
    pub fn new(
        ethereum_client: EthereumClient,
        gnt_contract_address: Address,
        db: DbExecutor,
    ) -> PaymentDriverResult<GntDriver> {
        let gnt_contract = GntDriver::prepare_contract(
            &ethereum_client,
            gnt_contract_address,
            include_bytes!("./contracts/gnt.json"),
        )?;

        Ok(GntDriver {
            ethereum_client: ethereum_client,
            gnt_contract: gnt_contract,
            db: db,
        })
    }

    /// Initializes testnet funds
    pub async fn init_funds(
        &self,
        address: Address,
        eth_faucet_address: &str,
        gnt_faucet_address: Address,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<()> {
        let max_testnet_balance = U256::from_dec_str(MAX_TESTNET_BALANCE).unwrap();

        if self.get_eth_balance(address)?.amount < max_testnet_balance {
            println!("Requesting Eth from Faucet...");
            self.request_eth_from_faucet(address, eth_faucet_address)
                .await?;
        } else {
            println!("To much Eth...");
        }

        // cannot have more than "10000000000000" Gnt
        // blocked by Faucet contract
        if self.get_gnt_balance(address)?.amount < max_testnet_balance {
            println!("Requesting Gnt from Faucet...");
            self.request_gnt_from_faucet(address, gnt_faucet_address, sign_tx)
                .await?;
        } else {
            println!("To much Gnt...");
        }

        Ok(())
    }

    /// Transfers Gnt
    pub async fn transfer_gnt(
        &self,
        amount: PaymentAmount,
        sender: Address,
        recipient: Address,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<H256> {
        let (gnt_amount, gas_amount) = self.prepare_payment_amounts(amount);

        if gnt_amount > self.get_gnt_balance(sender)?.amount {
            return Err(PaymentDriverError::InsufficientFunds);
        }

        let tx = self.prepare_raw_tx(
            sender,
            gas_amount,
            &self.gnt_contract,
            "transfer",
            (recipient, gnt_amount),
        )?;

        let tx_hash = self.send_raw_transaction(&tx, sign_tx).await?;
        Ok(tx_hash)
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
                |e| Err(PaymentDriverError::LibraryError(format!("{:?}", e))),
                |balance| Ok(Balance::new(balance, Currency::Gnt {})),
            )
    }

    /// Returns ether balance
    pub fn get_eth_balance(&self, address: Address) -> PaymentDriverResult<Balance> {
        let block_number = None;
        let amount = self
            .ethereum_client
            .get_eth_balance(address, block_number)?;
        Ok(Balance::new(amount, Currency::Eth {}))
    }

    /// Requests Eth from Faucet
    async fn request_eth_from_faucet(
        &self,
        address: Address,
        faucet_address: &str,
    ) -> PaymentDriverResult<()> {
        let sleep_time = time::Duration::from_secs(ETH_FAUCET_SLEEP_SECONDS);
        let mut counter = 0;
        while counter < MAX_ETH_FAUCET_REQUESTS {
            if self.request_eth(address, faucet_address).is_ok() {
                break;
            } else {
                println!("Failed to request Eth from Faucet...");
            }
            thread::sleep(sleep_time);
            counter += 1;
        }

        if counter < MAX_ETH_FAUCET_REQUESTS {
            Ok(())
        } else {
            Err(PaymentDriverError::LibraryError(format!(
                "Cannot request Eth from Faucet"
            )))
        }
    }

    fn request_eth(&self, address: Address, faucet_address: &str) -> Result<(), reqwest::Error> {
        let mut uri: String = faucet_address.into();
        uri.push('/');
        let addr: String = format!("{:x?}", address);
        uri.push_str(&addr.as_str()[2..]);
        println!("HTTP GET {:?}", uri);
        let _body = reqwest::blocking::get(uri.as_str())?.json::<HashMap<String, String>>()?;
        Ok(())
    }

    /// Requests Gnt from Faucet
    async fn request_gnt_from_faucet(
        &self,
        address: Address,
        faucet_contract_address: Address,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<()> {
        let contract = GntDriver::prepare_contract(
            &self.ethereum_client,
            faucet_contract_address,
            include_bytes!("./contracts/faucet.json"),
        )?;

        let tx =
            self.prepare_raw_tx(address, U256::from(GNT_FAUCET_GAS), &contract, "create", ())?;

        let tx_hash = self.send_raw_transaction(&tx, sign_tx).await?;

        println!("Tx hash: {:?}", tx_hash);
        Ok(())
    }

    async fn send_raw_transaction(
        &self,
        raw_tx: &RawTransaction,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<H256> {
        let chain_id = self.get_chain_id();
        let signature = sign_tx(raw_tx.hash(chain_id)).await;
        let signed_tx = raw_tx.encode_signed_tx(signature, chain_id);

        // TODO persistence
        let tx_hash = self.send_transaction(signed_tx)?;
        Ok(tx_hash)
    }

    fn check_gas_amount(
        &self,
        raw_tx: &RawTransaction,
        sender: Address,
    ) -> PaymentDriverResult<()> {
        let eth_balance = self.get_eth_balance(sender)?;
        if raw_tx.gas_price * raw_tx.gas > eth_balance.amount {
            Err(PaymentDriverError::InsufficientGas)
        } else {
            Ok(())
        }
    }

    fn prepare_raw_tx<P>(
        &self,
        sender: Address,
        gas: U256,
        contract: &Contract<Http>,
        func: &str,
        tokens: P,
    ) -> PaymentDriverResult<RawTransaction>
    where
        P: Tokenize,
    {
        let tx = RawTransaction {
            nonce: self.get_next_nonce(sender)?,
            to: Some(contract.address()),
            value: U256::from(0),
            gas_price: self.get_gas_price()?,
            gas: gas,
            data: contract.encode(func, tokens).unwrap(),
        };

        self.check_gas_amount(&tx, sender)?;

        Ok(tx)
    }

    fn prepare_contract(
        ethereum_client: &EthereumClient,
        address: Address,
        json_abi: &[u8],
    ) -> PaymentDriverResult<Contract<Http>> {
        let contract = ethereum_client.get_contract(address, json_abi)?;
        Ok(contract)
    }

    fn get_chain_id(&self) -> u64 {
        self.ethereum_client.get_chain_id()
    }

    fn send_transaction(&self, tx: Vec<u8>) -> PaymentDriverResult<H256> {
        let tx_hash = self.ethereum_client.send_tx(tx)?;
        Ok(tx_hash)
    }

    fn get_next_nonce(&self, address: Address) -> PaymentDriverResult<U256> {
        let nonce = self.ethereum_client.get_next_nonce(address)?;
        Ok(nonce)
    }

    fn get_gas_price(&self) -> PaymentDriverResult<U256> {
        let gas_price = self.ethereum_client.get_gas_price()?;
        Ok(gas_price)
    }

    fn prepare_payment_amounts(&self, amount: PaymentAmount) -> (U256, U256) {
        let gas_amount = if amount.gas_amount.is_some() {
            amount.gas_amount.unwrap()
        } else {
            U256::from(GNT_TRANSFER_GAS)
        };
        (amount.base_currency_amount, gas_amount)
    }

    #[allow(unused)]
    async fn save_transaction(&self, raw_tx: &RawTransaction, sender: Address) -> DbResult<()> {
        let entity = self.raw_tx_to_entity(raw_tx, sender);
        let dao: TransactionDao = self.db.as_dao();
        Ok(())
    }

    #[allow(unused)]
    fn raw_tx_to_entity(&self, raw_tx: &RawTransaction, sender: Address) -> TransactionEntity {
        // chain id always below i32 max value
        let chain_id = self.get_chain_id();

        let mut nonce_bytes = [0u8; 32];
        raw_tx.nonce.to_little_endian(&mut nonce_bytes);
        let nonce = hex::encode(nonce_bytes);

        TransactionEntity {
            tx_hash: hex::encode(raw_tx.hash(chain_id)),
            sender: hex::encode(sender),
            chain: chain_id as i32,
            nonce: nonce,
            timestamp: Utc::now().naive_utc(),
        }
    }

    #[allow(unused)]
    async fn update_payment_status(
        &self,
        invoice_id: String,
        status: PaymentStatus,
        tx_hash: Option<H256>,
    ) -> DbResult<()> {
        let tx_hash = match tx_hash {
            Some(hash) => Some(hex::encode(hash)),
            None => None,
        };

        let dao: PaymentDao = self.db.as_dao();
        dao.update_status(invoice_id, status.to_i32(), tx_hash)
            .await?;
        Ok(())
    }

    #[allow(unused)]
    async fn get_payment_from_db(&self, invoice_id: String) -> DbResult<Option<PaymentEntity>> {
        let dao: PaymentDao = self.db.as_dao();
        dao.get(invoice_id).await
    }
}

#[async_trait(?Send)]
impl PaymentDriver for GntDriver {
    /// Returns account balance
    async fn get_account_balance(&self, address: Address) -> PaymentDriverResult<AccountBalance> {
        let gnt_balance = self.get_gnt_balance(address)?;
        let eth_balance = self.get_eth_balance(address)?;

        Ok(AccountBalance::new(gnt_balance, Some(eth_balance)))
    }

    /// Schedules payment
    async fn schedule_payment(
        &mut self,
        _invoice_id: &str,
        amount: PaymentAmount,
        sender: Address,
        recipient: Address,
        _due_date: DateTime<Utc>,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<()> {
        let tx_hash = self
            .transfer_gnt(amount, sender, recipient, sign_tx)
            .await?;
        println!("Tx hash: {:?}", tx_hash);
        Ok(())
    }

    /// Returns payment status
    #[allow(unused)]
    async fn get_payment_status(&self, invoice_id: &str) -> PaymentDriverResult<PaymentStatus> {
        let payment = match self.get_payment_from_db(invoice_id.into()).await? {
            None => {
                return Ok(PaymentStatus::Unknown);
            }
            Some(payment) => payment,
        };
        Ok(PaymentStatus::from(payment))
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
    async fn get_transaction_balance(
        &self,
        payer: Address,
        payee: Address,
    ) -> PaymentDriverResult<Balance> {
        unimplemented!();
    }
}

#[cfg(test)]
mod tests {
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

    #[tokio::test]
    async fn test_new_driver() -> anyhow::Result<()> {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        let driver = GntDriver::new(
            ethereum_client,
            to_address(GNT_CONTRACT_ADDRESS),
            DbExecutor::new(":memory:")?,
        );
        assert!(driver.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn test_get_eth_balance() -> anyhow::Result<()> {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        let driver = GntDriver::new(
            ethereum_client,
            to_address(GNT_CONTRACT_ADDRESS),
            DbExecutor::new(":memory:")?,
        )
        .unwrap();
        let eth_balance = driver.get_eth_balance(to_address(ETH_ADDRESS));
        assert!(eth_balance.is_ok());
        let balance = eth_balance.unwrap();
        assert_eq!(balance.currency, Currency::Eth {});
        assert!(balance.amount >= U256::from(0));
        Ok(())
    }

    #[tokio::test]
    async fn test_get_gnt_balance() -> anyhow::Result<()> {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        let driver = GntDriver::new(
            ethereum_client,
            to_address(GNT_CONTRACT_ADDRESS),
            DbExecutor::new(":memory:")?,
        )
        .unwrap();
        let gnt_balance = driver.get_gnt_balance(to_address(ETH_ADDRESS));
        assert!(gnt_balance.is_ok());
        let balance = gnt_balance.unwrap();
        assert_eq!(balance.currency, Currency::Gnt {});
        assert!(balance.amount >= U256::from(0));
        Ok(())
    }

    #[tokio::test]
    async fn test_get_account_balance() -> anyhow::Result<()> {
        let (_eloop, transport) = Http::new(GETH_ADDRESS).unwrap();
        let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);
        let driver = GntDriver::new(
            ethereum_client,
            to_address(GNT_CONTRACT_ADDRESS),
            DbExecutor::new(":memory:")?,
        )
        .unwrap();

        let balance = driver
            .get_account_balance(to_address(ETH_ADDRESS))
            .await
            .unwrap();

        let gnt_balance = balance.base_currency;
        assert_eq!(gnt_balance.currency, Currency::Gnt {});
        assert!(gnt_balance.amount >= U256::from(0));

        let some_eth_balance = balance.gas;
        assert!(some_eth_balance.is_some());

        let eth_balance = some_eth_balance.unwrap();
        assert_eq!(eth_balance.currency, Currency::Eth {});
        assert!(eth_balance.amount >= U256::from(0));
        Ok(())
    }
}
