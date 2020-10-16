#[macro_use]
extern crate diesel;

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

mod dao;
mod error;
mod gnt;
mod models;
mod processor;
mod schema;
mod service;
mod utils;

pub use error::GNTDriverError;

use crate::dao::payment::PaymentDao;
use crate::gnt::ethereum::{Chain, EthereumClient, EthereumClientBuilder};
use crate::gnt::sender::{AccountLocked, AccountUnlocked};
use crate::gnt::{common, config, faucet, sender};
use crate::models::{PaymentEntity, TxType};
use crate::utils::PAYMENT_STATUS_NOT_YET;
use actix::Addr;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};

use crate::processor::GNTDriverProcessor;
use ethereum_types::{Address, H256, U256};
use futures3::prelude::*;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time;
use uuid::Uuid;
use web3::contract::Contract;
use web3::transports::Http;
use ya_client_model::NodeId;
use ya_core_model::driver::{AccountMode, PaymentConfirmation, PaymentDetails};
use ya_core_model::identity;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::Provider;
use ya_service_bus::typed as bus;

pub type GNTDriverResult<T> = Result<T, GNTDriverError>;

const GNT_FAUCET_GAS: u32 = 90000;
const CREATE_FAUCET_FUNCTION: &str = "create";

pub const PLATFORM_NAME: &'static str = "NGNT";
pub const DRIVER_NAME: &'static str = "ngnt";

const ETH_FAUCET_MAX_WAIT: time::Duration = time::Duration::from_secs(180);

async fn load_active_accounts(tx_sender: Addr<sender::TransactionSender>) -> GNTDriverResult<()> {
    log::info!("Load active accounts on driver start");
    match bus::service(identity::BUS_ID).call(identity::List {}).await {
        Err(e) => Err(GNTDriverError::LibraryError(format!(
            "Failed to list identities: {:?}",
            e
        ))),
        Ok(Err(e)) => Err(GNTDriverError::LibraryError(format!(
            "Failed to list identities: {:?}",
            e
        ))),
        Ok(Ok(identities)) => {
            log::debug!("Listed identities: {:?}", identities);
            for identity in identities {
                if !identity.is_locked {
                    let _ = tx_sender
                        .send(AccountUnlocked {
                            identity: identity.node_id,
                        })
                        .await
                        .map_err(|e| {
                            log::error!(
                                "Failed to add active identity ({:?}): {:?}",
                                identity.node_id,
                                e
                            )
                        });
                }
            }
            Ok(())
        }
    }
}

pub struct PaymentDriverService;

impl PaymentDriverService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        let db: DbExecutor = context.component();
        let driver = GntDriver::new(db.clone()).await?;
        let processor = GNTDriverProcessor::new(driver);
        self::service::bind_service(&db, processor);
        self::service::subscribe_to_identity_events().await;
        Ok(())
    }
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
    pub async fn new(db: DbExecutor) -> GNTDriverResult<GntDriver> {
        crate::dao::init(&db)
            .await
            .map_err(GNTDriverError::library_err_msg)?;
        let chain = Chain::from_env()?;
        let ethereum_client = Arc::new(EthereumClientBuilder::from_env()?.build()?);
        let env = config::EnvConfiguration::from_env(chain)?;

        let gnt_contract = Arc::new(common::prepare_gnt_contract(&ethereum_client, &env)?);
        let faucet_contract =
            common::prepare_gnt_faucet_contract(&ethereum_client, &env)?.map(Arc::new);
        let tx_sender = sender::TransactionSender::new(
            ethereum_client.clone(),
            gnt_contract.clone(),
            db.clone(),
            &env,
        );

        load_active_accounts(tx_sender.clone()).await?;

        Ok(GntDriver {
            db,
            ethereum_client,
            gnt_contract,
            faucet_contract,
            tx_sender,
        })
    }

    fn wait_for_eth<'a>(&self, address: Address) -> impl Future<Output = GNTDriverResult<()>> + 'a {
        let client = self.ethereum_client.clone();
        async move {
            log::info!("Waiting for ETH from faucet...");
            let wait_until = Utc::now() + chrono::Duration::from_std(ETH_FAUCET_MAX_WAIT).unwrap();
            while Utc::now() < wait_until {
                if client.get_balance(address).await? > U256::zero() {
                    log::info!("Received ETH from faucet.");
                    return Ok(());
                }
                tokio::time::delay_for(time::Duration::from_secs(3)).await;
            }
            log::error!("Waiting for ETH timed out.");
            Err(GNTDriverError::InsufficientFunds)
        }
    }

    /// Requests GNT from Faucet
    fn request_gnt_from_faucet<'a>(
        &self,
        address: Address,
    ) -> impl Future<Output = GNTDriverResult<()>> + 'a {
        let max_testnet_balance = utils::str_to_big_dec(config::MAX_TESTNET_BALANCE).unwrap();
        // cannot have more than "10000000000000" Gnt
        // blocked by Faucet contract
        let client = self.ethereum_client.clone();
        let sender = self.tx_sender.clone();
        let contract = self.gnt_contract.clone();
        let faucet_contract = self.faucet_contract.clone().unwrap();
        async move {
            let balance = common::get_gnt_balance(&contract, address).await?;
            if balance < max_testnet_balance {
                log::info!("Requesting NGNT from Faucet...");
                let gas_price = client.get_gas_price().await?;
                let mut b = sender::Builder::new(address, gas_price, client.chain_id())
                    .with_tx_type(TxType::Faucet);
                b.push(
                    &faucet_contract,
                    CREATE_FAUCET_FUNCTION,
                    (),
                    GNT_FAUCET_GAS.into(),
                );
                let sign_tx = utils::get_sign_tx(
                    NodeId::from_str(utils::addr_to_str(address).as_str()).unwrap(),
                );
                let resp = b.send_to(sender.clone(), &sign_tx).await?;
                log::info!("send new tx: {:?}", resp);
                for tx_id in resp {
                    let _ = sender.send(sender::WaitForTx { tx_id }).await??;
                }
            }
            Ok(())
        }
    }

    /// Returns sum of transactions from given address
    fn get_transaction_balance(
        &self,
        _payer: &str,
        _payee: &str,
    ) -> Pin<Box<dyn Future<Output = GNTDriverResult<BigDecimal>> + 'static>> {
        // TODO: Get real transaction balance
        Box::pin(future::ready(Ok(utils::str_to_big_dec(
            "1000000000000000000000000",
        )
        .unwrap())))
    }

    /// Initializes account
    fn init<'a>(
        &self,
        mode: AccountMode,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = GNTDriverResult<()>> + 'a>> {
        use futures3::prelude::*;

        let addr: String = address.into();
        Box::pin(
            if mode.contains(AccountMode::SEND)
                && self.ethereum_client.chain_id() == Chain::Rinkeby.id()
            {
                let address = utils::str_to_addr(address).unwrap();
                let wait_for_eth = self.wait_for_eth(address);
                let request_gnt = self.request_gnt_from_faucet(address);
                let fut = async move {
                    faucet::EthFaucetConfig::from_env()
                        .await?
                        .request_eth(address)
                        .await?;
                    wait_for_eth.await?;
                    request_gnt.await?;

                    gnt::register_account(addr, mode).await?;
                    Ok(())
                };

                fut.left_future()
            } else {
                let fut = async move {
                    gnt::register_account(addr, mode).await?;
                    Ok(())
                };
                fut.right_future()
            },
        )
    }

    /// Notification when identity gets locked and the driver cannot send transactions
    fn account_locked<'a>(
        &self,
        identity: NodeId,
    ) -> Pin<Box<dyn Future<Output = GNTDriverResult<()>> + 'a>> {
        let tx_sender = self.tx_sender.clone();
        Box::pin(async move { tx_sender.send(AccountLocked { identity }).await? })
    }

    /// Notification when identity gets unlocked and the driver can send transactions
    fn account_unlocked<'a>(
        &self,
        identity: NodeId,
    ) -> Pin<Box<dyn Future<Output = GNTDriverResult<()>> + 'a>> {
        let tx_sender = self.tx_sender.clone();
        Box::pin(async move { tx_sender.send(AccountUnlocked { identity }).await? })
    }

    /// Returns account balance
    fn get_account_balance(
        &self,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = GNTDriverResult<BigDecimal>> + 'static>> {
        let address: String = address.into();
        let gnt_contract = self.gnt_contract.clone();
        Box::pin(async move {
            let address = utils::str_to_addr(address.as_str())?;
            Ok(common::get_gnt_balance(&gnt_contract, address).await?)
        })
    }

    /// Schedules payment
    fn schedule_payment<'a>(
        &self,
        amount: BigDecimal,
        sender: &str,
        recipient: &str,
        due_date: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = GNTDriverResult<String>> + 'a>> {
        let db = self.db.clone();
        let order_id: String = format!("{}", Uuid::new_v4());
        let sender = sender.to_owned();
        let recipient = recipient.to_owned();
        let gnt_amount = utils::big_dec_to_u256(amount).unwrap();
        let gas_amount = Default::default();

        let payment = PaymentEntity {
            amount: utils::u256_to_big_endian_hex(gnt_amount),
            gas: utils::u256_to_big_endian_hex(gas_amount),
            order_id: order_id.clone(),
            payment_due_date: due_date.naive_utc(),
            sender: sender.clone(),
            recipient: recipient.clone(),
            status: PAYMENT_STATUS_NOT_YET,
            tx_id: None,
        };
        async move {
            db.as_dao::<PaymentDao>().insert(payment).await?;
            Ok(order_id)
        }
        .boxed_local()
    }

    /// Verifies payment
    fn verify_payment(
        &self,
        confirmation: &PaymentConfirmation,
    ) -> Pin<Box<dyn Future<Output = GNTDriverResult<PaymentDetails>> + 'static>> {
        let tx_hash: H256 = H256::from_slice(&confirmation.confirmation);
        let ethereum_client = self.ethereum_client.clone();
        let gnt_contract = self.gnt_contract.clone();
        Box::pin(async move {
            match ethereum_client.get_transaction_receipt(tx_hash).await? {
                None => Err(GNTDriverError::UnknownTransaction),
                Some(receipt) => {
                    common::verify_gnt_tx(&receipt, &gnt_contract)?;
                    common::build_payment_details(&receipt)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gnt::ethereum::*;
    use crate::utils;
    use ya_persistence::executor::DbExecutor;

    const ETH_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";

    #[ignore]
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
    async fn test_get_gnt_balance() -> anyhow::Result<()> {
        let ethereum_client = EthereumClientBuilder::with_chain(Chain::Rinkeby)?.build()?;
        let gnt_contract = common::prepare_gnt_contract(&ethereum_client, &config::CFG_TESTNET)?;
        let gnt_balance =
            common::get_gnt_balance(&gnt_contract, utils::str_to_addr(ETH_ADDRESS)?).await?;
        assert!(gnt_balance >= utils::str_to_big_dec("0")?);
        Ok(())
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_get_account_balance() -> anyhow::Result<()> {
        let driver = GntDriver::new(DbExecutor::new(":memory:")?).await.unwrap();
        let gnt_balance = driver.get_account_balance(ETH_ADDRESS).await.unwrap();
        assert!(gnt_balance >= utils::str_to_big_dec("0")?);
        Ok(())
    }

    #[ignore]
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
