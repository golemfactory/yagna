use async_trait::async_trait;

use chrono::{DateTime, Utc};

use ethereum_types::{Address, H160, H256, U256, U64};

use ethereum_tx_sign::RawTransaction;

use std::collections::HashMap;

use std::{thread, time};

use web3::contract::tokens::Tokenize;
use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::transports::Http;
use web3::types::{Bytes, Log, TransactionReceipt};

use ya_persistence::executor::DbExecutor;

use crate::account::{AccountBalance, Balance, Currency};
use crate::dao::payment::PaymentDao;
use crate::dao::transaction::TransactionDao;
use crate::error::{DbResult, PaymentDriverError};
use crate::ethereum::EthereumClient;
use crate::models::{PaymentEntity, TransactionEntity};
use crate::payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};
use crate::{AccountMode, PaymentDriver, PaymentDriverResult, SignTx};

const GNT_TRANSFER_GAS: u32 = 55000;
const GNT_FAUCET_GAS: u32 = 90000;

const MAX_ETH_FAUCET_REQUESTS: u32 = 10;
const ETH_FAUCET_SLEEP_SECONDS: u64 = 1;

const MAX_TESTNET_BALANCE: &str = "10000000000000";

const ETH_TX_SUCCESS: u64 = 1;
const TRANSFER_LOGS_LENGTH: usize = 1;
const TX_LOG_DATA_LENGTH: usize = 32;
const TX_LOG_TOPICS_LENGTH: usize = 3;
const TRANSFER_CANONICAL_SIGNATURE: &str =
    "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

pub struct GntDriver {
    ethereum_client: EthereumClient,
    gnt_contract: Contract<Http>,
    eth_faucet_address: String,
    gnt_faucet_address: Address,
    db: DbExecutor,
}

impl GntDriver {
    /// Creates new driver
    pub fn new(
        ethereum_client: EthereumClient,
        gnt_contract_address: Address,
        eth_faucet_address: impl Into<String>,
        gnt_faucet_address: Address,
        db: DbExecutor,
    ) -> PaymentDriverResult<GntDriver> {
        let gnt_contract = GntDriver::prepare_contract(
            &ethereum_client,
            gnt_contract_address,
            include_bytes!("./contracts/gnt.json"),
        )?;

        let eth_faucet_address = eth_faucet_address.into();
        Ok(GntDriver {
            ethereum_client,
            gnt_contract,
            eth_faucet_address,
            gnt_faucet_address,
            db,
        })
    }

    /// Initializes testnet funds
    pub async fn init_funds(
        &self,
        address: Address,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<()> {
        let max_testnet_balance = U256::from_dec_str(MAX_TESTNET_BALANCE).unwrap();

        if self.get_eth_balance(address)?.amount < max_testnet_balance {
            println!("Requesting Eth from Faucet...");
            self.request_eth_from_faucet(address).await?;
        } else {
            println!("To much Eth...");
        }

        // cannot have more than "10000000000000" Gnt
        // blocked by Faucet contract
        if self.get_gnt_balance(address)?.amount < max_testnet_balance {
            println!("Requesting Gnt from Faucet...");
            self.request_gnt_from_faucet(address, sign_tx).await?;
        } else {
            println!("To much Gnt...");
        }

        Ok(())
    }

    /// Transfers Gnt
    pub async fn transfer_gnt(
        &self,
        amount: &PaymentAmount,
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
    async fn request_eth_from_faucet(&self, address: Address) -> PaymentDriverResult<()> {
        let sleep_time = time::Duration::from_secs(ETH_FAUCET_SLEEP_SECONDS);
        let mut counter = 0;
        while counter < MAX_ETH_FAUCET_REQUESTS {
            if self.request_eth(address).await.is_ok() {
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

    async fn request_eth(&self, address: Address) -> Result<(), reqwest::Error> {
        let mut uri = self.eth_faucet_address.clone();
        uri.push('/');
        let addr: String = format!("{:x?}", address);
        uri.push_str(&addr.as_str()[2..]);
        println!("HTTP GET {:?}", uri);
        let _body = reqwest::get(uri.as_str())
            .await?
            .json::<HashMap<String, String>>()
            .await?;
        Ok(())
    }

    /// Requests Gnt from Faucet
    async fn request_gnt_from_faucet(
        &self,
        address: Address,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<()> {
        let contract = GntDriver::prepare_contract(
            &self.ethereum_client,
            self.gnt_faucet_address.clone(),
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

    fn prepare_payment_amounts(&self, amount: &PaymentAmount) -> (U256, U256) {
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

        let nonce = GntDriver::to_big_endian_hex(raw_tx.nonce);

        TransactionEntity {
            tx_hash: hex::encode(raw_tx.hash(chain_id)),
            sender: hex::encode(sender),
            chain: chain_id as i32,
            nonce: nonce,
            timestamp: Utc::now().naive_utc(),
        }
    }

    async fn add_payment<S>(
        &self,
        invoice_id: S,
        amount: &PaymentAmount,
        due_date: DateTime<Utc>,
        recipient: Address,
    ) -> PaymentDriverResult<()>
    where
        S: Into<String>,
    {
        let (gnt_amount, gas_amount) = self.prepare_payment_amounts(amount);

        let payment = PaymentEntity {
            amount: GntDriver::to_big_endian_hex(gnt_amount),
            gas: GntDriver::to_big_endian_hex(gas_amount),
            invoice_id: invoice_id.into(),
            payment_due_date: due_date.naive_utc(),
            recipient: hex::encode(recipient),
            status: PaymentStatus::NotYet.to_i32(),
            tx_hash: None,
        };

        let dao: PaymentDao = self.db.as_dao();
        dao.insert(payment).await?;
        Ok(())
    }

    fn to_big_endian_hex(value: U256) -> String {
        let mut bytes = [0u8; 32];
        value.to_big_endian(&mut bytes);
        hex::encode(&bytes)
    }

    #[allow(unused)]
    async fn update_payment_status<S>(&self, invoice_id: S, status: PaymentStatus) -> DbResult<()>
    where
        S: Into<String>,
    {
        let tx_hash = match &status {
            PaymentStatus::Ok(confirmation) => Some(hex::encode(&confirmation.confirmation)),
            _ => None,
        };

        let dao: PaymentDao = self.db.as_dao();
        dao.update_status(invoice_id.into(), status.to_i32(), tx_hash)
            .await?;

        Ok(())
    }

    async fn get_payment_from_db(&self, invoice_id: String) -> DbResult<Option<PaymentEntity>> {
        let dao: PaymentDao = self.db.as_dao();
        dao.get(invoice_id).await
    }

    fn verify_gnt_tx(&self, receipt: &TransactionReceipt) -> PaymentDriverResult<()> {
        self.verify_gnt_tx_logs(&receipt.logs)?;
        self.verify_gnt_tx_status(&receipt.status)?;
        Ok(())
    }

    fn verify_gnt_tx_status(&self, status: &Option<U64>) -> PaymentDriverResult<()> {
        match status {
            None => Err(PaymentDriverError::UnknownTransaction),
            Some(status) => {
                if *status == U64::from(ETH_TX_SUCCESS) {
                    Ok(())
                } else {
                    Err(PaymentDriverError::FailedTransaction)
                }
            }
        }
    }

    fn verify_gnt_tx_logs(&self, logs: &Vec<Log>) -> PaymentDriverResult<()> {
        if logs.len() != TRANSFER_LOGS_LENGTH {
            return Err(PaymentDriverError::UnknownTransaction);
        }
        self.verify_gnt_tx_log(&logs[0])?;
        Ok(())
    }

    fn verify_gnt_tx_log(&self, log: &Log) -> PaymentDriverResult<()> {
        self.verify_gnt_tx_log_contract_address(&log.address)?;
        self.verify_gnt_tx_log_topics(&log.topics)?;
        self.verify_gnt_tx_log_data(&log.data)?;
        Ok(())
    }

    fn verify_gnt_tx_log_contract_address(
        &self,
        contract_address: &Address,
    ) -> PaymentDriverResult<()> {
        if *contract_address != self.gnt_contract.address() {
            return Err(PaymentDriverError::UnknownTransaction);
        }
        Ok(())
    }

    fn verify_gnt_tx_log_topics(&self, topics: &Vec<H256>) -> PaymentDriverResult<()> {
        if topics.len() != TX_LOG_TOPICS_LENGTH {
            return Err(PaymentDriverError::UnknownTransaction);
        }
        // topics[0] is the keccak-256 of the Transfer(address,address,uint256) canonical signature
        self.verify_gnt_tx_log_canonical_signature(&topics[0])?;
        Ok(())
    }

    fn verify_gnt_tx_log_canonical_signature(
        &self,
        canonical_signature: &H256,
    ) -> PaymentDriverResult<()> {
        if *canonical_signature
            != H256::from_slice(&hex::decode(TRANSFER_CANONICAL_SIGNATURE).unwrap())
        {
            return Err(PaymentDriverError::UnknownTransaction);
        }
        Ok(())
    }

    fn verify_gnt_tx_log_data(&self, data: &Bytes) -> PaymentDriverResult<()> {
        if data.0.len() != TX_LOG_DATA_LENGTH {
            return Err(PaymentDriverError::UnknownTransaction);
        }
        Ok(())
    }

    fn build_payment_details(&self, receipt: &TransactionReceipt) -> PaymentDetails {
        // topics[1] is the value of the _from address as H256
        let sender = GntDriver::topic_to_address(&receipt.logs[0].topics[1]);
        // topics[2] is the value of the _to address as H256
        let recipient = GntDriver::topic_to_address(&receipt.logs[0].topics[2]);
        // The data field from the returned Log struct contains the transferred token amount value
        let amount: U256 = U256::from_big_endian(&receipt.logs[0].data.0);
        // Do not have any info about date in receipt
        let date = None;

        PaymentDetails {
            recipient,
            sender,
            amount,
            date,
        }
    }

    fn topic_to_address(topic: &H256) -> Address {
        H160::from_slice(&topic.as_bytes()[12..])
    }
}

#[async_trait(?Send)]
impl PaymentDriver for GntDriver {
    async fn init(
        &self,
        mode: AccountMode,
        address: Address,
        sign_tx: SignTx<'_>,
    ) -> Result<(), PaymentDriverError> {
        if mode.contains(AccountMode::SEND) {
            self.init_funds(address, sign_tx).await?;
        }
        Ok(())
    }

    /// Returns account balance
    async fn get_account_balance(&self, address: Address) -> PaymentDriverResult<AccountBalance> {
        let gnt_balance = self.get_gnt_balance(address)?;
        let eth_balance = self.get_eth_balance(address)?;

        Ok(AccountBalance::new(gnt_balance, Some(eth_balance)))
    }

    /// Schedules payment
    async fn schedule_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        sender: Address,
        recipient: Address,
        due_date: DateTime<Utc>,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<()> {
        // schedule payment
        self.add_payment(invoice_id, &amount, due_date, recipient)
            .await?;

        let tx_hash = self
            .transfer_gnt(&amount, sender, recipient, sign_tx)
            .await?;

        // update payment status
        // TODO uncomment after tx persistence
        // self.update_payment_status(
        //     invoice_id,
        //     PaymentStatus::Ok(PaymentConfirmation::from(tx_hash.as_bytes())),
        // )
        // .await?;

        println!("Tx hash: {:?}", tx_hash);
        Ok(())
    }

    /// Returns payment status
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
    async fn verify_payment(
        &self,
        confirmation: &PaymentConfirmation,
    ) -> PaymentDriverResult<PaymentDetails> {
        let tx_hash: H256 = H256::from_slice(&confirmation.confirmation);
        match self.ethereum_client.get_transaction_receipt(tx_hash)? {
            None => Err(PaymentDriverError::NotFound),
            Some(receipt) => {
                self.verify_gnt_tx(&receipt)?;
                Ok(self.build_payment_details(&receipt))
            }
        }
    }

    /// Returns sum of transactions from given address
    #[allow(unused)]
    async fn get_transaction_balance(
        &self,
        payer: Address,
        payee: Address,
    ) -> PaymentDriverResult<Balance> {
        // TODO: Get real transaction balance
        Ok(Balance {
            currency: Currency::Gnt,
            amount: U256::from_dec_str("1000000000000000000000000").unwrap(),
        })
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

    const ETH_FAUCET_ADDRESS: &str = "http://188.165.227.180:4000/donate";
    const GNT_FAUCET_ADDRESS: &str = "77b6145E853dfA80E8755a4e824c4F510ac6692e";

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
            ETH_FAUCET_ADDRESS,
            to_address(GNT_FAUCET_ADDRESS),
            DbExecutor::new(":memory:").unwrap(),
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
            ETH_FAUCET_ADDRESS,
            to_address(GNT_FAUCET_ADDRESS),
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
            ETH_FAUCET_ADDRESS,
            to_address(GNT_FAUCET_ADDRESS),
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
            ETH_FAUCET_ADDRESS,
            to_address(GNT_FAUCET_ADDRESS),
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
