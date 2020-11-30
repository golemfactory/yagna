use crate::GNTDriverResult;
use crate::GntDriver;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use ya_client_model::payment::Allocation;
use ya_client_model::NodeId;
use ya_core_model::driver::{AccountMode, PaymentConfirmation, PaymentDetails};

#[derive(Clone)]
pub struct GNTDriverProcessor {
    driver: Arc<GntDriver>,
}

impl GNTDriverProcessor {
    pub fn new(driver: GntDriver) -> Self {
        Self {
            driver: Arc::new(driver),
        }
    }

    pub async fn account_locked(&self, identity: NodeId) -> GNTDriverResult<()> {
        self.driver.account_locked(identity).await
    }

    pub async fn account_unlocked(&self, identity: NodeId) -> GNTDriverResult<()> {
        self.driver.account_unlocked(identity).await
    }

    pub async fn init(&self, mode: AccountMode, address: &str) -> GNTDriverResult<()> {
        self.driver.init(mode, address).await
    }

    pub async fn get_account_balance(&self, address: &str) -> GNTDriverResult<BigDecimal> {
        self.driver.get_account_balance(address).await
    }

    pub async fn get_transaction_balance(
        &self,
        sender: &str,
        recipient: &str,
    ) -> GNTDriverResult<BigDecimal> {
        self.driver.get_transaction_balance(sender, recipient).await
    }

    pub async fn schedule_payment(
        &self,
        amount: BigDecimal,
        sender: &str,
        recipient: &str,
        due_date: DateTime<Utc>,
    ) -> GNTDriverResult<String> {
        self.driver
            .schedule_payment(amount, sender, recipient, due_date)
            .await
    }

    pub async fn verify_payment(
        &self,
        confirmation: PaymentConfirmation,
    ) -> GNTDriverResult<PaymentDetails> {
        self.driver.verify_payment(&confirmation).await
    }

    pub async fn validate_allocation(
        &self,
        address: String,
        amount: BigDecimal,
        existing_allocations: Vec<Allocation>,
    ) -> GNTDriverResult<bool> {
        self.driver
            .validate_allocation(address, amount, existing_allocations)
            .await
    }
}
