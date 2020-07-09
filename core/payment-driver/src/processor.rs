use crate::PaymentDriver;
use crate::PaymentDriverResult;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use ya_client_model::NodeId;
use ya_core_model::driver::{
    AccountMode, PaymentConfirmation, PaymentDetails,
};

#[derive(Clone)]
pub struct PaymentDriverProcessor {
    driver: Arc<dyn PaymentDriver + Send + Sync + 'static>,
}

impl PaymentDriverProcessor {
    pub fn new<D>(driver: D) -> Self
    where
        D: PaymentDriver + Send + Sync + 'static,
    {
        Self {
            driver: Arc::new(driver),
        }
    }

    pub async fn account_locked(&self, identity: NodeId) -> PaymentDriverResult<()> {
        self.driver.account_locked(identity).await
    }

    pub async fn account_unlocked(&self, identity: NodeId) -> PaymentDriverResult<()> {
        self.driver.account_unlocked(identity).await
    }

    pub async fn init(&self, mode: AccountMode, address: &str) -> PaymentDriverResult<()> {
        self.driver.init(mode, address).await
    }

    pub async fn get_account_balance(&self, address: &str) -> PaymentDriverResult<BigDecimal> {
        self.driver.get_account_balance(address).await
    }

    pub async fn get_transaction_balance(
        &self,
        sender: &str,
        recipient: &str,
    ) -> PaymentDriverResult<BigDecimal> {
        self.driver.get_transaction_balance(sender, recipient).await
    }

    pub async fn schedule_payment(
        &self,
        amount: BigDecimal,
        sender: &str,
        recipient: &str,
        due_date: DateTime<Utc>,
    ) -> PaymentDriverResult<String> {
        self.driver
            .schedule_payment(amount, sender, recipient, due_date)
            .await
    }

    pub async fn verify_payment(
        &self,
        confirmation: PaymentConfirmation,
    ) -> PaymentDriverResult<PaymentDetails> {
        self.driver.verify_payment(&confirmation).await
    }
}
