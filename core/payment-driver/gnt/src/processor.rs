use crate::config::{MAINNET_CONFIG, RINKEBY_CONFIG};
use crate::networks::Network;
use crate::utils;
use crate::GNTDriverResult;
use crate::GntDriver;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use ya_client_model::payment::Allocation;
use ya_client_model::NodeId;
use ya_core_model::driver::{AccountMode, PaymentConfirmation, PaymentDetails};
use ya_persistence::executor::DbExecutor;

#[derive(Clone)]
pub struct GNTDriverProcessor {
    rinkeby_driver: Arc<GntDriver>,
    mainnet_driver: Arc<GntDriver>,
}

impl GNTDriverProcessor {
    pub async fn new(db: DbExecutor) -> GNTDriverResult<Self> {
        Ok(Self {
            rinkeby_driver: Arc::new(
                GntDriver::new(db.clone(), Network::Rinkeby, *RINKEBY_CONFIG).await?,
            ),
            mainnet_driver: Arc::new(
                GntDriver::new(db.clone(), Network::Mainnet, *MAINNET_CONFIG).await?,
            ),
        })
    }

    pub async fn account_locked(&self, identity: NodeId) -> GNTDriverResult<()> {
        self.rinkeby_driver.account_locked(identity).await?;
        self.mainnet_driver.account_locked(identity).await
    }

    pub async fn account_unlocked(&self, identity: NodeId) -> GNTDriverResult<()> {
        self.rinkeby_driver.account_unlocked(identity).await?;
        self.mainnet_driver.account_unlocked(identity).await
    }

    pub async fn fund(&self, address: &str, network: Network) -> GNTDriverResult<String> {
        match network {
            Network::Rinkeby => {
                let address = utils::str_to_addr(address)?;
                self.rinkeby_driver.fund(address).await
            },
            Network::Mainnet => { Ok(format!(
                "Your mainnet ethereum address is {}. Send some GLM tokens and ETH for gas to this address \
                to be able to use this driver. Using this driver is not recommended. If you want to easily \
                acquire some GLM to try Golem on mainnet please use zksync driver.", address
            )) }
        }
    }

    fn driver(&self, network: Network) -> &Arc<GntDriver> {
        match network {
            Network::Rinkeby => &self.rinkeby_driver,
            Network::Mainnet => &self.mainnet_driver,
        }
    }

    pub async fn init(
        &self,
        mode: AccountMode,
        address: &str,
        network: Network,
    ) -> GNTDriverResult<()> {
        self.driver(network).init(mode, address).await
    }

    pub async fn get_account_balance(
        &self,
        address: &str,
        network: Network,
    ) -> GNTDriverResult<BigDecimal> {
        self.driver(network).get_account_balance(address).await
    }

    pub async fn schedule_payment(
        &self,
        amount: BigDecimal,
        sender: &str,
        recipient: &str,
        network: Network,
        due_date: DateTime<Utc>,
    ) -> GNTDriverResult<String> {
        self.driver(network)
            .schedule_payment(amount, sender, recipient, due_date)
            .await
    }

    pub async fn verify_payment(
        &self,
        confirmation: PaymentConfirmation,
        network: Network,
    ) -> GNTDriverResult<PaymentDetails> {
        self.driver(network).verify_payment(&confirmation).await
    }

    pub async fn validate_allocation(
        &self,
        address: String,
        network: Network,
        amount: BigDecimal,
        existing_allocations: Vec<Allocation>,
    ) -> GNTDriverResult<bool> {
        self.driver(network)
            .validate_allocation(address, amount, existing_allocations)
            .await
    }
}
