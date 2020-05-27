use actix::prelude::*;
use chrono::{DateTime, Utc};
use ethereum_types::{Address, H256, U256, U64};

use web3::contract::{Contract, Options};
use web3::transports::Http;
use web3::types::{Bytes, Log, TransactionReceipt};

use ya_persistence::executor::DbExecutor;

use crate::dao::payment::PaymentDao;

use crate::error::PaymentDriverError;
use crate::ethereum::{Chain, EthereumClient, EthereumClientBuilder};
use crate::models::{PaymentEntity, TxType};
use crate::{PaymentDriver, PaymentDriverResult, SignTx};

use futures3::compat::*;
use futures3::prelude::*;

use crate::utils;

use std::future::Future;
use std::pin::Pin;

use crate::utils::{
    payment_status_to_i32, PAYMENT_STATUS_FAILED, PAYMENT_STATUS_NOT_ENOUGH_FUNDS,
    PAYMENT_STATUS_NOT_ENOUGH_GAS,
};
use std::sync::Arc;
use web3::Transport;
use ya_core_model::driver::{
    AccountBalance, AccountMode, Balance, Currency, PaymentAmount, PaymentConfirmation,
    PaymentDetails, PaymentStatus,
};

mod config;
mod faucet;
mod sender;

const GNT_TRANSFER_GAS: u32 = 55000;
const GNT_FAUCET_GAS: u32 = 90000;

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

fn prepare_gnt_contract(
    ethereum_client: &EthereumClient,
    env: &config::EnvConfiguration,
) -> PaymentDriverResult<Contract<Http>> {
    prepare_contract(
        ethereum_client,
        env.gnt_contract_address,
        include_bytes!("./contracts/gnt2.json"),
    )
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
    env: &config::EnvConfiguration,
) -> PaymentDriverResult<Option<Contract<Http>>> {
    if let Some(gnt_faucet_address) = env.gnt_faucet_address {
        Ok(Some(prepare_contract(
            ethereum_client,
            gnt_faucet_address,
            include_bytes!("./contracts/faucet.json"),
        )?))
    } else {
        Ok(None)
    }
}

fn verify_gnt_tx<T: Transport>(
    receipt: &TransactionReceipt,
    contract: &Contract<T>,
) -> PaymentDriverResult<()> {
    verify_gnt_tx_logs(&receipt.logs, contract)?;
    verify_gnt_tx_status(&receipt.status)?;
    Ok(())
}

fn verify_gnt_tx_status(status: &Option<U64>) -> PaymentDriverResult<()> {
    match status {
        None => Err(PaymentDriverError::UnknownTransaction),
        Some(status) => {
            if *status == U64::from(config::ETH_TX_SUCCESS) {
                Ok(())
            } else {
                Err(PaymentDriverError::FailedTransaction)
            }
        }
    }
}

fn verify_gnt_tx_logs<T: Transport>(
    logs: &Vec<Log>,
    contract: &Contract<T>,
) -> PaymentDriverResult<()> {
    if logs.len() != config::TRANSFER_LOGS_LENGTH {
        return Err(PaymentDriverError::UnknownTransaction);
    }
    verify_gnt_tx_log(&logs[0], contract)?;
    Ok(())
}

fn verify_gnt_tx_log<T: Transport>(log: &Log, contract: &Contract<T>) -> PaymentDriverResult<()> {
    if log.address != contract.address() {
        return Err(PaymentDriverError::UnknownTransaction);
    }
    verify_gnt_tx_log_topics(&log.topics)?;
    verify_gnt_tx_log_data(&log.data)?;
    Ok(())
}

fn verify_gnt_tx_log_topics(topics: &Vec<H256>) -> PaymentDriverResult<()> {
    if topics.len() != config::TX_LOG_TOPICS_LENGTH {
        return Err(PaymentDriverError::UnknownTransaction);
    }
    // topics[0] is the keccak-256 of the Transfer(address,address,uint256) canonical signature
    verify_gnt_tx_log_canonical_signature(&topics[0])?;
    Ok(())
}

fn verify_gnt_tx_log_canonical_signature(canonical_signature: &H256) -> PaymentDriverResult<()> {
    if *canonical_signature
        != H256::from_slice(&hex::decode(config::TRANSFER_CANONICAL_SIGNATURE).unwrap())
    {
        return Err(PaymentDriverError::UnknownTransaction);
    }
    Ok(())
}

fn verify_gnt_tx_log_data(data: &Bytes) -> PaymentDriverResult<()> {
    if data.0.len() != config::TX_LOG_DATA_LENGTH {
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

pub struct GntDriver {
    db: DbExecutor,
    ethereum_client: Arc<EthereumClient>,
    gnt_contract: Arc<Contract<Http>>,
    faucet_contract: Option<Arc<Contract<Http>>>,
    tx_sender: Addr<sender::TransactionSender>,
}

impl GntDriver {
    /// Creates new driver
    pub async fn new(db: DbExecutor) -> PaymentDriverResult<GntDriver> {
        crate::dao::init(&db)
            .await
            .map_err(PaymentDriverError::library_err_msg)?;
        let chain = Chain::from_env()?;
        let ethereum_client = Arc::new(EthereumClientBuilder::from_env()?.build()?);
        let env = config::EnvConfiguration::from_env(chain)?;

        let gnt_contract = Arc::new(prepare_gnt_contract(&ethereum_client, &env)?);
        let faucet_contract = prepare_gnt_faucet_contract(&ethereum_client, &env)?.map(Arc::new);
        let tx_sender = sender::TransactionSender::new(ethereum_client.clone(), db.clone());

        Ok(GntDriver {
            db,
            ethereum_client,
            gnt_contract,
            faucet_contract,
            tx_sender,
        })
    }

    /// Requests Gnt from Faucet
    fn request_gnt_from_faucet<'a>(
        &self,
        address: Address,
        sign_tx: SignTx<'a>,
    ) -> impl Future<Output = PaymentDriverResult<()>> + 'a {
        let max_testnet_balance = utils::str_to_big_dec(config::MAX_TESTNET_BALANCE).unwrap();
        // cannot have more than "10000000000000" Gnt
        // blocked by Faucet contract
        let client = self.ethereum_client.clone();
        let sender = self.tx_sender.clone();
        let contract = self.gnt_contract.clone();
        let faucet_contract = self.faucet_contract.clone().unwrap();
        async move {
            let balance = get_gnt_balance(&contract, address).await?;
            if balance.amount < max_testnet_balance {
                log::info!("Requesting Gnt from Faucet...");
                let gas_price = client.get_gas_price().await?;
                let mut b = sender::Builder::new(address, gas_price, client.chain_id())
                    .with_tx_type(TxType::Faucet);
                b.push(&faucet_contract, "create", (), GNT_FAUCET_GAS.into());
                let resp = b.send_to(sender.clone(), sign_tx).await?;
                log::info!("send new tx: {:?}", resp);
                for tx_id in resp {
                    let _ = sender.send(sender::WaitForTx { tx_id }).await??;
                }
            }
            Ok(())
        }
    }
}

impl PaymentDriver for GntDriver {
    fn init<'a>(
        &self,
        mode: AccountMode,
        address: &str,
        sign_tx: SignTx<'a>,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'a>> {
        use futures3::prelude::*;

        Box::pin(
            if mode.contains(AccountMode::SEND)
                && self.ethereum_client.chain_id() == Chain::Rinkeby.id()
            {
                let address = utils::str_to_addr(address).unwrap();
                let req = self.request_gnt_from_faucet(address, sign_tx);
                let fut = async move {
                    faucet::EthFaucetConfig::from_env()?
                        .request_eth(address)
                        .await?;
                    req.await?;
                    Ok(())
                };

                fut.left_future()
            } else {
                future::ok(()).right_future()
            },
        )
    }

    /// Returns account balance
    fn get_account_balance(
        &self,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<AccountBalance>> + 'static>> {
        let address: String = address.into();
        let ethereum_client = self.ethereum_client.clone();
        let gnt_contract = self.gnt_contract.clone();
        Box::pin(async move {
            let address = utils::str_to_addr(address.as_str())?;
            let (eth_balance, gnt_balance) = future::try_join(
                get_eth_balance(&ethereum_client, address),
                get_gnt_balance(&gnt_contract, address),
            )
            .await?;
            Ok(AccountBalance::new(gnt_balance, Some(eth_balance)))
        })
    }

    /// Schedules payment
    fn schedule_payment<'a>(
        &self,
        invoice_id: &str,
        amount: PaymentAmount,
        sender: &str,
        recipient: &str,
        due_date: DateTime<Utc>,
        sign_tx: SignTx<'a>,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'a>> {
        let db = self.db.clone();
        let client = self.ethereum_client.clone();
        let invoice_id = invoice_id.to_owned();
        let sender = sender.to_owned();
        let recipient = recipient.to_owned();
        let gnt_amount = utils::big_dec_to_u256(amount.base_currency_amount).unwrap();
        let gas_amount = Default::default();
        let gnt_contract = self.gnt_contract.clone();
        let tx_sender = self.tx_sender.clone();

        let payment = PaymentEntity {
            amount: utils::u256_to_big_endian_hex(gnt_amount),
            gas: utils::u256_to_big_endian_hex(gas_amount),
            invoice_id: invoice_id.clone(),
            payment_due_date: due_date.naive_utc(),
            sender: sender.clone(),
            recipient: recipient.clone(),
            status: payment_status_to_i32(&PaymentStatus::NotYet {}),
            tx_id: None,
        };
        async move {
            db.as_dao::<PaymentDao>().insert(payment).await?;
            let gas_price = client.get_gas_price().await?;
            let chain_id = client.chain_id();
            match transfer_gnt(
                gnt_contract,
                tx_sender,
                gnt_amount,
                utils::str_to_addr(&sender)?,
                utils::str_to_addr(&recipient)?,
                sign_tx,
                gas_price,
                chain_id,
            )
            .await
            {
                Ok(tx_id) => {
                    db.as_dao::<PaymentDao>()
                        .update_tx_id(invoice_id, tx_id)
                        .await?;
                }
                Err(e) => {
                    db.as_dao::<PaymentDao>()
                        .update_status(
                            invoice_id,
                            match e {
                                PaymentDriverError::InsufficientFunds => {
                                    PAYMENT_STATUS_NOT_ENOUGH_FUNDS
                                }
                                PaymentDriverError::InsufficientGas => {
                                    PAYMENT_STATUS_NOT_ENOUGH_GAS
                                }
                                _ => PAYMENT_STATUS_FAILED,
                            },
                        )
                        .await?;
                    log::error!("gnt transfer failed: {}", e);
                    return Err(e);
                }
            }

            Ok(())
        }
        .boxed_local()
    }

    /// Reschedules payment
    fn reschedule_payment<'a>(
        &self,
        invoice_id: &str,
        _sign_tx: SignTx<'a>,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'a>> {
        let db = self.db.clone();
        let tx_sender = self.tx_sender.clone();
        let invoice_id = invoice_id.to_owned();
        async move {
            let payment = match db.as_dao::<PaymentDao>().get(invoice_id.clone()).await? {
                Some(v) => v,
                None => return Err(PaymentDriverError::PaymentNotFound(invoice_id)),
            };
            if let Some(tx_id) = payment.tx_id {
                tx_sender.send(sender::Retry { tx_id }).await??;
            }
            Ok(())
        }
        .boxed_local()
    }

    /// Returns payment status
    fn get_payment_status(
        &self,
        invoice_id: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<PaymentStatus>> + 'static>> {
        let invoice_id = invoice_id.to_owned();
        let db = self.db.clone();
        async move {
            if let Some(status) = db
                .as_dao::<PaymentDao>()
                .get_payment_status(invoice_id.clone())
                .await?
            {
                Ok(status)
            } else {
                Err(PaymentDriverError::UnknownPayment(invoice_id))
            }
        }
        .boxed_local()
    }

    /// Verifies payment
    fn verify_payment(
        &self,
        confirmation: &PaymentConfirmation,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<PaymentDetails>> + 'static>> {
        let tx_hash: H256 = H256::from_slice(&confirmation.confirmation);
        let ethereum_client = self.ethereum_client.clone();
        let gnt_contract = self.gnt_contract.clone();
        Box::pin(async move {
            match ethereum_client.get_transaction_receipt(tx_hash).await? {
                None => Err(PaymentDriverError::UnknownTransaction),
                Some(receipt) => {
                    verify_gnt_tx(&receipt, &gnt_contract)?;
                    build_payment_details(&receipt)
                }
            }
        })
    }

    /// Returns sum of transactions from given address
    fn get_transaction_balance(
        &self,
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

async fn transfer_gnt(
    gnt_contract: Arc<Contract<Http>>,
    tx_sender: Addr<sender::TransactionSender>,
    gnt_amount: U256,
    address: Address,
    recipient: Address,
    sign_tx: SignTx<'_>,
    gas_price: U256,
    chain_id: u64,
) -> PaymentDriverResult<String> {
    if gnt_amount > utils::big_dec_to_u256(get_gnt_balance(&gnt_contract, address).await?.amount)? {
        return Err(PaymentDriverError::InsufficientFunds);
    }

    let mut batch = sender::Builder::new(address, gas_price, chain_id);
    batch.push(
        &gnt_contract,
        "transfer",
        (recipient, gnt_amount),
        GNT_TRANSFER_GAS.into(),
    );
    let r = batch.send_to(tx_sender, sign_tx).await?;
    Ok(r.into_iter().next().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils;
    use ya_core_model::driver::Currency;

    const ETH_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";

    #[actix_rt::test]
    async fn test_new_driver() -> anyhow::Result<()> {
        {
            let driver = GntDriver::new(DbExecutor::new(":memory:").unwrap()).await;
            assert!(driver.is_ok());
        }

        tokio::time::delay_for(std::time::Duration::from_millis(5)).await;
        Ok(())
    }

    #[actix_rt::test]
    async fn test_get_eth_balance() -> anyhow::Result<()> {
        let ethereum_client = EthereumClientBuilder::with_chain(Chain::Rinkeby)?.build()?;
        let eth_balance =
            get_eth_balance(&ethereum_client, utils::str_to_addr(ETH_ADDRESS)?).await?;
        assert_eq!(eth_balance.currency, Currency::Eth {});
        assert!(eth_balance.amount >= utils::str_to_big_dec("0")?);
        Ok(())
    }

    #[actix_rt::test]
    async fn test_get_gnt_balance() -> anyhow::Result<()> {
        let ethereum_client = EthereumClientBuilder::with_chain(Chain::Rinkeby)?.build()?;
        let gnt_contract = prepare_gnt_contract(&ethereum_client, &config::CFG_TESTNET)?;
        let gnt_balance = get_gnt_balance(&gnt_contract, utils::str_to_addr(ETH_ADDRESS)?).await?;
        assert_eq!(gnt_balance.currency, Currency::Gnt {});
        assert!(gnt_balance.amount >= utils::str_to_big_dec("0")?);
        Ok(())
    }

    #[actix_rt::test]
    async fn test_get_account_balance() -> anyhow::Result<()> {
        let driver = GntDriver::new(DbExecutor::new(":memory:")?).await.unwrap();

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

    #[actix_rt::test]
    async fn test_verify_payment() -> anyhow::Result<()> {
        let driver = GntDriver::new(DbExecutor::new(":memory:")?).await.unwrap();
        let tx_hash: Vec<u8> =
            hex::decode("bb7f9fbf3fd08e75f1f3bda035b8d3109edce96dc6bab5624503146217a79c24")
                .unwrap();
        let confirmation = PaymentConfirmation::from(&tx_hash);

        let expected = PaymentDetails {
            recipient: String::from("0xf466400dd3c7ef0694205c2e93754ffce7c32313"),
            sender: String::from("0xf466400dd3c7ef0694205c2e93754ffce7c32313"),
            amount: utils::str_to_big_dec("69")?,
            date: None,
        };
        let details = driver.verify_payment(&confirmation).await?;
        assert_eq!(details, expected);
        Ok(())
    }
}
