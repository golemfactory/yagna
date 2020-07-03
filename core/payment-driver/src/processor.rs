use crate::PaymentDriver;
use crate::PaymentDriverResult;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use ya_client_model::NodeId;
use ya_core_model::driver::{
    AccountBalance, AccountMode, Balance, PaymentAmount, PaymentConfirmation, PaymentDetails,
    PaymentStatus,
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

    pub async fn get_account_balance(&self, address: &str) -> PaymentDriverResult<AccountBalance> {
        self.driver.get_account_balance(address).await
    }

    pub async fn get_payment_status(&self, invoice_id: &str) -> PaymentDriverResult<PaymentStatus> {
        self.driver.get_payment_status(invoice_id).await
    }

    pub async fn get_transaction_balance(
        &self,
        sender: &str,
        recipient: &str,
    ) -> PaymentDriverResult<Balance> {
        self.driver.get_transaction_balance(sender, recipient).await
    }

    pub async fn schedule_payment(
        &self,
        invoice_id: &str,
        amount: PaymentAmount,
        sender: &str,
        recipient: &str,
        due_date: DateTime<Utc>,
    ) -> PaymentDriverResult<()> {
        self.driver
            .schedule_payment(invoice_id, amount, sender, recipient, due_date)
            .await
    }

    pub async fn verify_payment(
        &self,
        confirmation: PaymentConfirmation,
    ) -> PaymentDriverResult<PaymentDetails> {
        self.driver.verify_payment(&confirmation).await
    }
}
