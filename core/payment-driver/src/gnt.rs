use chrono::{DateTime, Utc};

use ethereum_types::{Address, H256, U256, U64};

use ethereum_tx_sign::RawTransaction;

use std::collections::HashMap;
use std::sync::Arc;
use std::{thread, time};
use tokio::sync::{mpsc, oneshot};
use web3::contract::tokens::Tokenize;
use web3::contract::{Contract, Options};
use web3::transports::Http;
use web3::types::{Bytes, Log, TransactionReceipt};

use ya_persistence::executor::DbExecutor;

use crate::account::{AccountBalance, Balance, Currency};
use crate::dao::payment::PaymentDao;
use crate::dao::transaction::TransactionDao;
use crate::error::{DbResult, PaymentDriverError};
use crate::ethereum::{Chain, EthereumClient};
use crate::models::{PaymentEntity, TransactionEntity, TransactionStatus};
use crate::payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};
use crate::{AccountMode, PaymentDriver, PaymentDriverResult, SignTx};

use futures3::compat::*;

use crate::utils;
use futures3::future;
use std::env;
use std::future::Future;
use std::pin::Pin;
const GNT_TRANSFER_GAS: u32 = 55000;
const GNT_FAUCET_GAS: u32 = 90000;

const MAX_ETH_FAUCET_REQUESTS: u32 = 10;
const ETH_FAUCET_SLEEP_SECONDS: u64 = 1;

const MAX_TESTNET_BALANCE: &str = "1000";

const ETH_TX_SUCCESS: u64 = 1;
const TRANSFER_LOGS_LENGTH: usize = 1;
const TX_LOG_DATA_LENGTH: usize = 32;
const TX_LOG_TOPICS_LENGTH: usize = 3;
const TRANSFER_CANONICAL_SIGNATURE: &str =
    "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

const GNT_CONTRACT_ADDRESS_ENV_KEY: &str = "GNT_CONTRACT_ADDRESS";
const GNT_FAUCET_CONTRACT_ADDRESS_ENV_KEY: &str = "FAUCET_CONTRACT_ADDRESS";
const ETH_FAUCET_ADDRESS_ENV_KEY: &str = "ETH_FAUCET_ADDRESS";

const TX_SENDER_BUFFER: usize = 100;

const REQUIRED_CONFIRMATIONS: usize = 5;

async fn get_eth_balance(
    ethereum_client: &EthereumClient,
    address: Address,
) -> PaymentDriverResult<Balance> {
    let block_number = None;
    let amount = ethereum_client
        .get_eth_balance(address, block_number)
        .await?;
    Ok(Balance::new(
        utils::u256_to_big_dec(amount)?,
        Currency::Eth {},
    ))
}

async fn get_gnt_balance(
    gnt_contract: &Contract<Http>,
    address: Address,
) -> PaymentDriverResult<Balance> {
    gnt_contract
        .query("balanceOf", (address,), None, Options::default(), None)
        .compat()
        .await
        .map_or_else(
            |e| Err(PaymentDriverError::LibraryError(format!("{:?}", e))),
            |amount| {
                Ok(Balance::new(
                    utils::u256_to_big_dec(amount)?,
                    Currency::Gnt {},
                ))
            },
        )
}

fn prepare_gnt_contract(ethereum_client: &EthereumClient) -> PaymentDriverResult<Contract<Http>> {
    let contract_address = get_gnt_contract_address()?;
    prepare_contract(
        ethereum_client,
        contract_address,
        include_bytes!("./contracts/gnt.json"),
    )
}

fn get_gnt_contract_address() -> PaymentDriverResult<Address> {
    get_contract_address(GNT_CONTRACT_ADDRESS_ENV_KEY)
}

fn get_contract_address(env_key: &str) -> PaymentDriverResult<Address> {
    let contract_address: Address = utils::str_to_addr(env::var(env_key)?.as_str())?;
    Ok(contract_address)
}

fn prepare_contract(
    ethereum_client: &EthereumClient,
    address: Address,
    json_abi: &[u8],
) -> PaymentDriverResult<Contract<Http>> {
    let contract = ethereum_client.get_contract(address, json_abi)?;
    Ok(contract)
}

fn prepare_gnt_faucet_contract(
    ethereum_client: &EthereumClient,
) -> PaymentDriverResult<Contract<Http>> {
    let contract_address = get_gnt_faucet_contract_address()?;
    prepare_contract(
        ethereum_client,
        contract_address,
        include_bytes!("./contracts/faucet.json"),
    )
}

fn get_gnt_faucet_contract_address() -> PaymentDriverResult<Address> {
    get_contract_address(GNT_FAUCET_CONTRACT_ADDRESS_ENV_KEY)
}

fn verify_gnt_tx(receipt: &TransactionReceipt) -> PaymentDriverResult<()> {
    verify_gnt_tx_logs(&receipt.logs)?;
    verify_gnt_tx_status(&receipt.status)?;
    Ok(())
}

fn verify_gnt_tx_status(status: &Option<U64>) -> PaymentDriverResult<()> {
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

fn verify_gnt_tx_logs(logs: &Vec<Log>) -> PaymentDriverResult<()> {
    if logs.len() != TRANSFER_LOGS_LENGTH {
        return Err(PaymentDriverError::UnknownTransaction);
    }
    verify_gnt_tx_log(&logs[0])?;
    Ok(())
}

fn verify_gnt_tx_log(log: &Log) -> PaymentDriverResult<()> {
    verify_gnt_tx_log_contract_address(&log.address)?;
    verify_gnt_tx_log_topics(&log.topics)?;
    verify_gnt_tx_log_data(&log.data)?;
    Ok(())
}

fn verify_gnt_tx_log_contract_address(contract_address: &Address) -> PaymentDriverResult<()> {
    if *contract_address != get_gnt_contract_address()? {
        return Err(PaymentDriverError::UnknownTransaction);
    }
    Ok(())
}

fn verify_gnt_tx_log_topics(topics: &Vec<H256>) -> PaymentDriverResult<()> {
    if topics.len() != TX_LOG_TOPICS_LENGTH {
        return Err(PaymentDriverError::UnknownTransaction);
    }
    // topics[0] is the keccak-256 of the Transfer(address,address,uint256) canonical signature
    verify_gnt_tx_log_canonical_signature(&topics[0])?;
    Ok(())
}

fn verify_gnt_tx_log_canonical_signature(canonical_signature: &H256) -> PaymentDriverResult<()> {
    if *canonical_signature != H256::from_slice(&hex::decode(TRANSFER_CANONICAL_SIGNATURE).unwrap())
    {
        return Err(PaymentDriverError::UnknownTransaction);
    }
    Ok(())
}

fn verify_gnt_tx_log_data(data: &Bytes) -> PaymentDriverResult<()> {
    if data.0.len() != TX_LOG_DATA_LENGTH {
        return Err(PaymentDriverError::UnknownTransaction);
    }
    Ok(())
}

fn build_payment_details(receipt: &TransactionReceipt) -> PaymentDriverResult<PaymentDetails> {
    // topics[1] is the value of the _from address as H256
    let sender = utils::topic_to_address(&receipt.logs[0].topics[1]);
    // topics[2] is the value of the _to address as H256
    let recipient = utils::topic_to_address(&receipt.logs[0].topics[2]);
    // The data field from the returned Log struct contains the transferred token amount value
    let amount: U256 = utils::u256_from_big_endian(&receipt.logs[0].data.0);
    // Do not have any info about date in receipt
    let date = None;

    Ok(PaymentDetails {
        recipient: utils::addr_to_str(recipient).into(),
        sender: utils::addr_to_str(sender).into(),
        amount: utils::u256_to_big_dec(amount)?,
        date,
    })
}

async fn request_eth_from_faucet(address: Address) -> PaymentDriverResult<()> {
    log::info!("Requesting Eth from Faucet...");
    let sleep_time = time::Duration::from_secs(ETH_FAUCET_SLEEP_SECONDS);
    let mut counter = 0;
    let eth_faucet_address = env::var(ETH_FAUCET_ADDRESS_ENV_KEY)?;
    while counter < MAX_ETH_FAUCET_REQUESTS {
        let res = request_eth(address, &eth_faucet_address).await;
        if res.is_ok() {
            break;
        } else {
            log::error!("Failed to request Eth from Faucet: {:?}", res);
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

async fn request_eth(address: Address, eth_faucet_address: &String) -> PaymentDriverResult<()> {
    let uri = format!(
        "{}/{}",
        eth_faucet_address.clone(),
        utils::addr_to_str(address)
    );

    ureq::get(uri.as_str()).call().into_string().map_or_else(
        |e| Err(PaymentDriverError::LibraryError(format!("{:?}", e))),
        |resp| {
            log::debug!("{}", resp);
            // Sufficient funds
            if resp.contains("sufficient funds") {
                Ok(())
            }
            // Funds requested
            else if resp.contains("txhash") {
                Ok(())
            } else {
                Err(PaymentDriverError::LibraryError(resp))
            }
        },
    )
}

async fn check_gas_amount(
    ethereum_client: &EthereumClient,
    raw_tx: &RawTransaction,
    sender: Address,
) -> PaymentDriverResult<()> {
    if raw_tx.gas_price * raw_tx.gas
        > utils::big_dec_to_u256(get_eth_balance(ethereum_client, sender).await?.amount)?
    {
        Err(PaymentDriverError::InsufficientGas)
    } else {
        Ok(())
    }
}

fn prepare_payment_amounts(amount: PaymentAmount) -> PaymentDriverResult<(U256, U256)> {
    let gas_amount = match amount.gas_amount {
        Some(gas_amount) => utils::big_dec_to_u256(gas_amount)?,
        None => U256::from(GNT_TRANSFER_GAS),
    };
    Ok((
        utils::big_dec_to_u256(amount.base_currency_amount)?,
        gas_amount,
    ))
}

async fn confirm_tx(db: &DbExecutor, tx: RawTransaction, tx_hash: H256) -> PaymentDriverResult<()> {
    let ethereum_client = EthereumClient::new().expect("Failed to start ethereum client");
    ethereum_client
        .wait_for_confirmations(tx_hash, REQUIRED_CONFIRMATIONS)
        .await?;
    let receipt = ethereum_client.get_transaction_receipt(tx_hash).await?;
    let (tx_status, payment_status) =
        if receipt.unwrap().status.unwrap() == U64::from(ETH_TX_SUCCESS) {
            log::error!("Tx: {:?} confirmed", tx_hash);
            (
                TransactionStatus::Confirmed.into(),
                PaymentStatus::Ok(PaymentConfirmation {
                    confirmation: vec![0],
                })
                .to_i32(),
            )
        } else {
            log::error!("Tx: {:?} failed", tx_hash);
            (
                TransactionStatus::Failed.into(),
                PaymentStatus::Failed.to_i32(),
            )
        };
    let tx_id = tx.hash(ethereum_client.get_chain_id());
    update_tx_status(db, &tx_id, tx_status).await?;
    update_payment_status_by_tx_id(db, &tx_id, payment_status).await
}

async fn update_tx_status(
    db: &DbExecutor,
    tx_id: &Vec<u8>,
    tx_status: i32,
) -> PaymentDriverResult<()> {
    let dao: TransactionDao = db.as_dao();
    dao.update_tx_status(hex::encode(tx_id), tx_status).await?;
    Ok(())
}

async fn update_payment_status_by_tx_id(
    db: &DbExecutor,
    tx_id: &Vec<u8>,
    status: i32,
) -> PaymentDriverResult<()> {
    let dao: PaymentDao = db.as_dao();
    dao.update_status_by_tx_id(hex::encode(tx_id), status)
        .await?;
    Ok(())
}
async fn update_tx_sent(db: &DbExecutor, tx_id: Vec<u8>, tx_hash: H256) -> PaymentDriverResult<()> {
    let dao: TransactionDao = db.as_dao();
    dao.update_tx_sent(hex::encode(tx_id), hex::encode(&tx_hash))
        .await?;
    Ok(())
}

pub struct GntDriver {
    db: Arc<DbExecutor>,
    ethereum_client: EthereumClient,
    gnt_contract: Contract<Http>,
    nonces: HashMap<Address, U256>,
    tx_sender: mpsc::Sender<(
        (RawTransaction, Vec<u8>),
        oneshot::Sender<PaymentDriverResult<H256>>,
    )>,
}

impl GntDriver {
    /// Creates new driver
    pub fn new(db: DbExecutor) -> PaymentDriverResult<GntDriver> {
        let db = Arc::new(db);
        // TODO
        let migrate_db = db.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::dao::init(&migrate_db).await {
                log::error!("gnt migration error: {}", e);
            }
        });

        let ethereum_client = EthereumClient::new()?;

        let gnt_contract = prepare_gnt_contract(&ethereum_client)?;

        let (tx_sender, mut tx_sender_service) = mpsc::channel::<(
            (RawTransaction, Vec<u8>),
            oneshot::Sender<PaymentDriverResult<H256>>,
        )>(TX_SENDER_BUFFER);

        let sender_db = db.clone();
        tokio::spawn(async move {
            let db = sender_db;
            let ethereum_client = EthereumClient::new().expect("Failed to prepare Ethereum client");
            let chain_id: u64 = ethereum_client.get_chain_id();
            while let Some(((raw_tx, signature), response)) = tx_sender_service.recv().await {
                let result = ethereum_client
                    .send_tx(raw_tx.encode_signed_tx(signature, chain_id))
                    .await;
                response.send(result.clone()).unwrap();
                match result {
                    Ok(tx_hash) => {
                        let db = db.clone();
                        tokio::spawn(async move {
                            let _ = update_tx_sent(&db, raw_tx.hash(chain_id), tx_hash)
                                .await
                                .unwrap();
                            let _ = confirm_tx(&db, raw_tx, tx_hash).await.unwrap();
                        });
                        log::error!("Tx hash: {:?}", tx_hash);
                    }
                    Err(e) => {
                        log::error!("Failed to send tx: {:?}", e);
                    }
                }
            }
        });

        Ok(GntDriver {
            db,
            ethereum_client,
            gnt_contract,
            nonces: HashMap::new(),
            tx_sender,
        })
    }

    /// Initializes testnet funds
    async fn init_funds(&mut self, address: &str, sign_tx: SignTx<'_>) -> PaymentDriverResult<()> {
        let address = utils::str_to_addr(address)?;
        request_eth_from_faucet(address).await?;
        self.request_gnt_from_faucet(address, sign_tx).await?;
        Ok(())
    }

    /// Requests Gnt from Faucet
    async fn request_gnt_from_faucet(
        &mut self,
        address: Address,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<()> {
        let max_testnet_balance = utils::str_to_big_dec(MAX_TESTNET_BALANCE)?;
        // cannot have more than "10000000000000" Gnt
        // blocked by Faucet contract
        if get_gnt_balance(&self.gnt_contract, address).await?.amount < max_testnet_balance {
            log::info!("Requesting Gnt from Faucet...");
            let contract = prepare_gnt_faucet_contract(&self.ethereum_client)?;
            self.add_tx(
                address,
                U256::from(GNT_FAUCET_GAS),
                &contract,
                "create",
                (),
                sign_tx,
            )
            .await?;
        }
        Ok(())
    }

    async fn add_tx<P>(
        &mut self,
        sender: Address,
        gas: U256,
        contract: &Contract<Http>,
        func: &str,
        tokens: P,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<String>
    where
        P: Tokenize,
    {
        let chain_id = self.get_chain_id();
        let raw_tx = self
            .prepare_raw_tx(sender, gas, contract, func, tokens)
            .await?;
        let tx_hash = raw_tx.hash(chain_id);
        let signature = sign_tx(tx_hash.clone()).await;
        // increment nonce
        (*self.nonces.entry(sender).or_insert(raw_tx.nonce)) += U256::from(1u64);
        log::debug!("{:?}", raw_tx);
        self.save_transaction(&raw_tx, sender, &signature).await?;
        self.send_tx(raw_tx, signature).await?;
        Ok(hex::encode(&tx_hash))
    }

    async fn prepare_raw_tx<P>(
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
            nonce: self.get_next_nonce(sender).await?,
            to: Some(contract.address()),
            value: U256::from(0),
            gas_price: self.get_gas_price().await?,
            gas: gas,
            data: contract.encode(func, tokens).unwrap(),
        };

        check_gas_amount(&self.ethereum_client, &tx, sender).await?;

        Ok(tx)
    }

    async fn get_next_nonce(&self, address: Address) -> PaymentDriverResult<U256> {
        match self.nonces.get(&address) {
            Some(next_nonce) => Ok(*next_nonce),
            None => Ok(self.fetch_next_nonce(address).await?),
        }
    }

    async fn fetch_next_nonce(&self, address: Address) -> PaymentDriverResult<U256> {
        let geth_nonce = self.ethereum_client.get_next_nonce(address).await?;
        let db_nonce = self.get_next_nonce_from_db(address).await?;
        Ok(std::cmp::max(geth_nonce, db_nonce))
    }

    async fn get_next_nonce_from_db(&self, address: Address) -> PaymentDriverResult<U256> {
        let dao: TransactionDao = self.db.as_dao();
        let nonces: Vec<U256> = dao
            .get_used_nonces(utils::addr_to_str(address))
            .await?
            .into_iter()
            .map(|nonce| utils::u256_from_big_endian_hex(nonce))
            .collect();
        match nonces.iter().max() {
            None => Ok(U256::from(0)),
            Some(last_nonce) => Ok(last_nonce + U256::from(1)),
        }
    }
    async fn get_gas_price(&self) -> PaymentDriverResult<U256> {
        let gas_price = self.ethereum_client.get_gas_price().await?;
        Ok(gas_price)
    }

    async fn save_transaction(
        &self,
        raw_tx: &RawTransaction,
        sender: Address,
        signature: &Vec<u8>,
    ) -> DbResult<()> {
        let entity =
            utils::raw_tx_to_entity(raw_tx, sender, self.get_chain_id(), Utc::now(), signature);
        let dao: TransactionDao = self.db.as_dao();
        dao.insert(entity).await?;
        Ok(())
    }

    async fn send_tx(&self, raw_tx: RawTransaction, signature: Vec<u8>) -> PaymentDriverResult<()> {
        let mut tx_sender = self.tx_sender.clone();
        tokio::spawn(async move {
            let (resp_tx, resp_rx) = oneshot::channel();
            tx_sender
                .send(((raw_tx, signature), resp_tx))
                .await
                .ok()
                .unwrap();
            let tx_hash = resp_rx.await.unwrap().unwrap();
            log::info!("Sent tx: {:?}", tx_hash);
        });
        Ok(())
    }

    fn get_chain_id(&self) -> u64 {
        self.ethereum_client.get_chain_id()
    }

    async fn add_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        sender: &str,
        recipient: &str,
        due_date: DateTime<Utc>,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<()> {
        if self.get_payment_from_db(invoice_id.into()).await?.is_some() {
            return Err(PaymentDriverError::PaymentAlreadyScheduled(
                invoice_id.into(),
            ));
        }

        let (gnt_amount, gas_amount) = prepare_payment_amounts(amount)?;

        let mut payment = PaymentEntity {
            amount: utils::u256_to_big_endian_hex(gnt_amount),
            gas: utils::u256_to_big_endian_hex(gas_amount),
            invoice_id: invoice_id.into(),
            payment_due_date: due_date.naive_utc(),
            sender: sender.into(),
            recipient: recipient.into(),
            status: PaymentStatus::NotYet.to_i32(),
            tx_id: None,
        };

        let (status, tx_id) = self
            .transfer_gnt(
                gnt_amount,
                gas_amount,
                utils::str_to_addr(sender)?,
                utils::str_to_addr(recipient)?,
                sign_tx,
            )
            .await
            .map_or_else(
                |error| {
                    let status = match error {
                        PaymentDriverError::InsufficientFunds => {
                            PaymentStatus::NotEnoughFunds.to_i32()
                        }
                        PaymentDriverError::InsufficientGas => PaymentStatus::NotEnoughGas.to_i32(),
                        _ => PaymentStatus::Failed.to_i32(),
                    };
                    (status, None)
                },
                |tx_id| (PaymentStatus::NotYet.to_i32(), Some(tx_id)),
            );

        payment.status = status;
        payment.tx_id = tx_id;
        let dao: PaymentDao = self.db.as_dao();
        dao.insert(payment).await?;
        Ok(())
    }

    async fn transfer_gnt(
        &mut self,
        gnt_amount: U256,
        gas_amount: U256,
        sender: Address,
        recipient: Address,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<String> {
        if gnt_amount
            > utils::big_dec_to_u256(get_gnt_balance(&self.gnt_contract, sender).await?.amount)?
        {
            return Err(PaymentDriverError::InsufficientFunds);
        }

        self.add_tx(
            sender,
            gas_amount,
            &self.gnt_contract.clone(),
            "transfer",
            (recipient, gnt_amount),
            sign_tx,
        )
        .await
    }

    async fn get_payment_from_db(
        &self,
        invoice_id: String,
    ) -> PaymentDriverResult<Option<PaymentEntity>> {
        let dao: PaymentDao = self.db.as_dao();
        let payment = dao.get(invoice_id).await?;
        Ok(payment)
    }

    async fn fetch_payment_status(&self, invoice_id: &str) -> PaymentDriverResult<PaymentStatus> {
        match self.get_payment_from_db(invoice_id.into()).await? {
            Some(payment) => self.map_payment_to_payment_status(payment).await,
            None => Ok(PaymentStatus::Unknown),
        }
    }

    async fn map_payment_to_payment_status(
        &self,
        payment: PaymentEntity,
    ) -> PaymentDriverResult<PaymentStatus> {
        let tx_id = payment.tx_id.clone();
        let tx = if tx_id.is_some() {
            self.get_tx_from_db(tx_id.unwrap()).await?
        } else {
            None
        };
        let tx_hash = match tx {
            None => None,
            Some(tx) => tx.tx_hash,
        };

        let status: PaymentStatus = payment.into();
        match status {
            PaymentStatus::Ok(_) => match tx_hash {
                Some(tx_hash) => {
                    let hash: Vec<u8> = hex::decode(tx_hash).unwrap();
                    Ok(PaymentStatus::Ok(PaymentConfirmation {
                        confirmation: hash,
                    }))
                }
                // tx hash cannot be empty
                None => Ok(PaymentStatus::Failed),
            },
            status => Ok(status),
        }
    }

    async fn get_tx_from_db(
        &self,
        tx_id: String,
    ) -> PaymentDriverResult<Option<TransactionEntity>> {
        let dao: TransactionDao = self.db.as_dao();
        let tx_entity = dao.get(tx_id).await?;
        Ok(tx_entity)
    }

    async fn retry_payment(
        &mut self,
        invoice_id: &str,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<()> {
        match self.get_payment_from_db(invoice_id.into()).await? {
            None => Err(PaymentDriverError::UnknownPayment(invoice_id.into())),
            Some(payment) => {
                let gnt_amount = utils::u256_from_big_endian_hex(payment.amount);
                let gas_amount = utils::u256_from_big_endian_hex(payment.gas);
                let sender = utils::str_to_addr(payment.sender.as_str())?;
                let recipient = utils::str_to_addr(payment.recipient.as_str())?;
                let result = self
                    .transfer_gnt(gnt_amount, gas_amount, sender, recipient, sign_tx)
                    .await;
                match result {
                    Err(error) => {
                        let status = match error {
                            PaymentDriverError::InsufficientFunds => {
                                PaymentStatus::NotEnoughFunds.to_i32()
                            }
                            PaymentDriverError::InsufficientGas => {
                                PaymentStatus::NotEnoughGas.to_i32()
                            }
                            _ => PaymentStatus::Failed.to_i32(),
                        };
                        self.update_payment_status(invoice_id.into(), status)
                            .await?;
                    }
                    Ok(tx_id) => {
                        self.update_payment_status(
                            invoice_id.into(),
                            PaymentStatus::NotYet.to_i32(),
                        )
                        .await?;
                        self.update_payment_tx_id(invoice_id.into(), tx_id).await?;
                    }
                };
                Ok(())
            }
        }
    }

    async fn update_payment_status(
        &self,
        invoice_id: String,
        status: i32,
    ) -> PaymentDriverResult<()> {
        let dao: PaymentDao = self.db.as_dao();
        dao.update_status(invoice_id, status).await?;
        Ok(())
    }

    async fn update_payment_tx_id(
        &self,
        invoice_id: String,
        tx_id: String,
    ) -> PaymentDriverResult<()> {
        let dao: PaymentDao = self.db.as_dao();
        dao.update_tx_id(invoice_id, Some(tx_id)).await?;
        Ok(())
    }
}

impl PaymentDriver for GntDriver {
    fn init<'a>(
        &'a mut self,
        mode: AccountMode,
        address: &str,
        sign_tx: SignTx,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'static>> {
        let result = if mode.contains(AccountMode::SEND)
            && self.ethereum_client.get_chain_id() == Chain::Rinkeby.id()
        {
            futures3::executor::block_on(self.init_funds(address, sign_tx))
        } else {
            Ok(())
        };
        Box::pin(future::ready(result))
    }

    /// Returns account balance
    fn get_account_balance<'a>(
        &'a self,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<AccountBalance>> + 'static>> {
        let address: String = address.into();
        Box::pin(async move {
            let address = utils::str_to_addr(address.as_str())?;
            let ethereum_client = EthereumClient::new()?;
            let gnt_contract = prepare_gnt_contract(&ethereum_client)?;
            let eth_balance = get_eth_balance(&ethereum_client, address).await?;
            let gnt_balance = get_gnt_balance(&gnt_contract, address).await?;
            Ok(AccountBalance::new(gnt_balance, Some(eth_balance)))
        })
    }

    /// Schedules payment
    fn schedule_payment<'a>(
        &'a mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        sender: &str,
        recipient: &str,
        due_date: DateTime<Utc>,
        sign_tx: SignTx,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'static>> {
        let result = futures3::executor::block_on(
            self.add_payment(invoice_id, amount, sender, recipient, due_date, sign_tx),
        );
        Box::pin(future::ready(result))
    }

    /// Reschedules payment
    fn reschedule_payment<'a>(
        &'a mut self,
        invoice_id: &str,
        sign_tx: SignTx,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'static>> {
        let result = futures3::executor::block_on(self.retry_payment(invoice_id, sign_tx));
        Box::pin(future::ready(result))
    }

    /// Returns payment status
    fn get_payment_status<'a>(
        &'a self,
        invoice_id: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<PaymentStatus>> + 'static>> {
        let result = futures3::executor::block_on(self.fetch_payment_status(invoice_id));
        Box::pin(future::ready(result))
    }

    /// Verifies payment
    fn verify_payment<'a>(
        &'a self,
        confirmation: &PaymentConfirmation,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<PaymentDetails>> + 'static>> {
        let tx_hash: H256 = H256::from_slice(&confirmation.confirmation);
        Box::pin(async move {
            let ethereum_client = EthereumClient::new()?;
            match ethereum_client.get_transaction_receipt(tx_hash).await? {
                None => Err(PaymentDriverError::UnknownTransaction),
                Some(receipt) => {
                    verify_gnt_tx(&receipt)?;
                    build_payment_details(&receipt)
                }
            }
        })
    }

    /// Returns sum of transactions from given address
    fn get_transaction_balance<'a>(
        &'a self,
        _payer: &str,
        _payee: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<Balance>> + 'static>> {
        // TODO: Get real transaction balance
        Box::pin(future::ready(Ok(Balance {
            currency: Currency::Gnt,
            amount: utils::str_to_big_dec("1000000000000000000000000").unwrap(),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::Currency;
    use crate::ethereum::Chain;
    use crate::utils;
    use std::sync::Once;

    static INIT: Once = Once::new();

    const ETH_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";

    fn init_env() {
        INIT.call_once(|| {
            std::env::set_var("GETH_ADDRESS", "http://1.geth.testnet.golem.network:55555");
            std::env::set_var("CHAIN_ID", format!("{:?}", Chain::Rinkeby.id()));
            std::env::set_var(
                "GNT_CONTRACT_ADDRESS",
                "0x924442A66cFd812308791872C4B242440c108E19",
            );
            std::env::set_var(
                "FAUCET_CONTRACT_ADDRESS",
                "0x77b6145E853dfA80E8755a4e824c4F510ac6692e",
            );
            std::env::set_var("ETH_FAUCET_ADDRESS", "http://faucet.testnet.golem.network:4000/donate");
        });
    }

    #[tokio::test]
    async fn test_new_driver() -> anyhow::Result<()> {
        init_env();
        let driver = GntDriver::new(DbExecutor::new(":memory:").unwrap());
        assert!(driver.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn test_get_eth_balance() -> anyhow::Result<()> {
        init_env();
        let ethereum_client = EthereumClient::new()?;
        let eth_balance =
            get_eth_balance(&ethereum_client, utils::str_to_addr(ETH_ADDRESS)?).await?;
        assert_eq!(eth_balance.currency, Currency::Eth {});
        assert!(eth_balance.amount >= utils::str_to_big_dec("0")?);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_gnt_balance() -> anyhow::Result<()> {
        init_env();
        let ethereum_client = EthereumClient::new()?;
        let gnt_contract = prepare_gnt_contract(&ethereum_client)?;
        let gnt_balance = get_gnt_balance(&gnt_contract, utils::str_to_addr(ETH_ADDRESS)?).await?;
        assert_eq!(gnt_balance.currency, Currency::Gnt {});
        assert!(gnt_balance.amount >= utils::str_to_big_dec("0")?);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_account_balance() -> anyhow::Result<()> {
        init_env();
        let driver = GntDriver::new(DbExecutor::new(":memory:")?).unwrap();

        let balance = driver.get_account_balance(ETH_ADDRESS).await.unwrap();

        let gnt_balance = balance.base_currency;
        assert_eq!(gnt_balance.currency, Currency::Gnt {});
        assert!(gnt_balance.amount >= utils::str_to_big_dec("0")?);

        let some_eth_balance = balance.gas;
        assert!(some_eth_balance.is_some());

        let eth_balance = some_eth_balance.unwrap();
        assert_eq!(eth_balance.currency, Currency::Eth {});
        assert!(eth_balance.amount >= utils::str_to_big_dec("0")?);
        Ok(())
    }

    #[tokio::test]
    async fn test_verify_payment() -> anyhow::Result<()> {
        init_env();
        let driver = GntDriver::new(DbExecutor::new(":memory:")?).unwrap();
        let tx_hash: Vec<u8> =
            hex::decode("df06916d8a8fe218e6261d3e811b1d9aee9cf8e07fb539431f0433abcdd9a8c2")
                .unwrap();
        let confirmation = PaymentConfirmation::from(&tx_hash);

        let expected = PaymentDetails {
            recipient: String::from("0x43a5b798e0e78be13b7bc0c553e433fb5b639be5"),
            sender: String::from("0x43a5b798e0e78be13b7bc0c553e433fb5b639be5"),
            amount: utils::str_to_big_dec("0.00000000000001")?,
            date: None,
        };
        let details = driver.verify_payment(&confirmation).await?;
        assert_eq!(details, expected);
        Ok(())
    }
}
