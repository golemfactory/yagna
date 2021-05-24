#[macro_use]
extern crate diesel;

#[macro_use]
extern crate num_derive;

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

mod dao;
mod error;
mod eth_utils;
mod gnt;
mod models;
mod networks;
mod processor;
mod schema;
mod service;
mod utils;

pub use error::GNTDriverError;

use crate::dao::payment::PaymentDao;
use crate::gnt::ethereum::{EthereumClient, EthereumClientBuilder};
use crate::gnt::sender::{AccountLocked, AccountUnlocked};
use crate::gnt::{common, config, faucet, sender};
use crate::models::{PaymentEntity, TxType};
use crate::networks::Network;
use crate::utils::PAYMENT_STATUS_NOT_YET;
use actix::Addr;
use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, Utc};

use crate::gnt::config::EnvConfiguration;
use crate::processor::GNTDriverProcessor;
use ethereum_types::{Address, H256, U256};
use futures3::prelude::*;
use lazy_static::lazy_static;
use maplit::hashmap;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time;
use uuid::Uuid;
use web3::contract::Contract;
use web3::transports::Http;
use ya_client_model::payment::Allocation;
use ya_client_model::NodeId;
use ya_core_model::driver::{AccountMode, PaymentConfirmation, PaymentDetails};
use ya_core_model::identity;
use ya_core_model::payment::local::DriverDetails;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::Provider;
use ya_service_bus::typed as bus;

pub type GNTDriverResult<T> = Result<T, GNTDriverError>;

const GNT_FAUCET_GAS: u32 = 90000;
const CREATE_FAUCET_FUNCTION: &str = "create";

pub const DRIVER_NAME: &'static str = "erc20";

lazy_static! {
    pub static ref DRIVER_DETAILS: DriverDetails = DriverDetails {
        default_network: Network::default().to_string(),
        networks: hashmap! {
            Network::Mainnet.to_string() => Network::Mainnet.into(),
            Network::Rinkeby.to_string() => Network::Rinkeby.into(),
        },
        recv_init_required: false,
    };
}

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
        let processor = GNTDriverProcessor::new(db.clone()).await?;
        self::service::bind_service(&db, processor);
        self::service::subscribe_to_identity_events().await?;
        self::service::register_in_payment_service().await?;
        Ok(())
    }
}

pub struct GntDriver {
    db: DbExecutor,
    network: Network,
    ethereum_client: Arc<EthereumClient>,
    gnt_contract: Arc<Contract<Http>>,
    faucet_contract: Option<Arc<Contract<Http>>>,
    tx_sender: Addr<sender::TransactionSender>,
}

impl GntDriver {
    /// Creates new driver
    pub async fn new(
        db: DbExecutor,
        network: Network,
        config: EnvConfiguration,
    ) -> GNTDriverResult<GntDriver> {
        crate::dao::init(&db)
            .await
            .map_err(GNTDriverError::library_err_msg)?;
        let ethereum_client = Arc::new(EthereumClientBuilder::with_network(network).build()?);

        let gnt_contract = Arc::new(common::prepare_gnt_contract(&ethereum_client, &config)?);
        let faucet_contract =
            common::prepare_gnt_faucet_contract(&ethereum_client, &config)?.map(Arc::new);
        let tx_sender = sender::TransactionSender::new(
            network,
            ethereum_client.clone(),
            gnt_contract.clone(),
            db.clone(),
            &config,
        );

        load_active_accounts(tx_sender.clone()).await?;

        Ok(GntDriver {
            db,
            network,
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
                tokio::time::sleep(time::Duration::from_secs(3)).await;
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
        let chain_id = self.network.chain_id();
        async move {
            let balance = common::get_gnt_balance(&contract, address).await?;
            if balance < max_testnet_balance {
                log::info!("Requesting tGLM from Faucet...");
                let gas_price = client.get_gas_price().await?;
                let mut b =
                    sender::Builder::new(address, gas_price, chain_id).with_tx_type(TxType::Faucet);
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

    /// Obtains funds from faucet
    fn fund<'a>(
        &self,
        address: Address,
    ) -> Pin<Box<dyn Future<Output = GNTDriverResult<String>> + 'a>> {
        let wait_for_eth = self.wait_for_eth(address);
        let request_gnt_from_faucet = self.request_gnt_from_faucet(address);
        Box::pin(async move {
            faucet::EthFaucetConfig::from_env()
                .await?
                .request_eth(address)
                .await?;
            wait_for_eth.await?;
            request_gnt_from_faucet.await?;

            Ok("Funds obtained from faucet.".to_string())
        })
    }

    /// Initializes account
    fn init<'a>(
        &self,
        mode: AccountMode,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = GNTDriverResult<()>> + 'a>> {
        let address = address.to_string();
        let network = self.network.to_string();
        let token = self.network.default_token();
        let gnt_contract = self.gnt_contract.clone();
        let eth_client = self.ethereum_client.clone();

        Box::pin(async move {
            if mode.contains(AccountMode::SEND) {
                let h160_addr = utils::str_to_addr(&address)?;

                let gnt_balance = common::get_gnt_balance(&gnt_contract, h160_addr).await?;
                if gnt_balance == BigDecimal::zero() {
                    return Err(GNTDriverError::InsufficientFunds);
                }

                let eth_balance = eth_client.get_balance(h160_addr).await?;
                if eth_balance == U256::zero() {
                    return Err(GNTDriverError::InsufficientGas);
                }
            }

            gnt::register_account(address, network, token, mode).await
        })
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
            network: self.network,
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

    fn validate_allocation(
        &self,
        address: String,
        amount: BigDecimal,
        existing_allocations: Vec<Allocation>,
    ) -> Pin<Box<dyn Future<Output = GNTDriverResult<bool>> + 'static>> {
        let gnt_contract = self.gnt_contract.clone();
        Box::pin(async move {
            let address = utils::str_to_addr(address.as_str())?;
            let balance = common::get_gnt_balance(&gnt_contract, address).await?;
            let total_allocated_amount: BigDecimal = existing_allocations
                .into_iter()
                .map(|allocation| allocation.remaining_amount)
                .sum();
            Ok(amount <= (balance - total_allocated_amount))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gnt::config::RINKEBY_CONFIG;
    use crate::utils;
    use ya_persistence::executor::DbExecutor;

    const ETH_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";

    #[ignore]
    #[actix_rt::test]
    async fn test_new_driver() -> anyhow::Result<()> {
        {
            let driver = GntDriver::new(
                DbExecutor::new(":memory:").unwrap(),
                Network::Rinkeby,
                *RINKEBY_CONFIG,
            )
            .await;
            assert!(driver.is_ok());
        }

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        Ok(())
    }

    #[actix_rt::test]
    async fn test_get_gnt_balance() -> anyhow::Result<()> {
        let ethereum_client = EthereumClientBuilder::with_network(Network::Rinkeby).build()?;
        let gnt_contract = common::prepare_gnt_contract(&ethereum_client, &config::RINKEBY_CONFIG)?;
        let gnt_balance =
            common::get_gnt_balance(&gnt_contract, utils::str_to_addr(ETH_ADDRESS)?).await?;
        assert!(gnt_balance >= utils::str_to_big_dec("0")?);
        Ok(())
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_get_account_balance() -> anyhow::Result<()> {
        let driver = GntDriver::new(
            DbExecutor::new(":memory:")?,
            Network::Rinkeby,
            *RINKEBY_CONFIG,
        )
        .await
        .unwrap();
        let gnt_balance = driver.get_account_balance(ETH_ADDRESS).await.unwrap();
        assert!(gnt_balance >= utils::str_to_big_dec("0")?);
        Ok(())
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_verify_payment() -> anyhow::Result<()> {
        let driver = GntDriver::new(
            DbExecutor::new(":memory:")?,
            Network::Rinkeby,
            *RINKEBY_CONFIG,
        )
        .await
        .unwrap();
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
