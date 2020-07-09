use crate::processor::PaymentDriverProcessor;
use chrono::{DateTime, Utc};
use ya_client_model::NodeId;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::Provider;

#[macro_use]
extern crate diesel;

mod dummy;
mod ethereum;
mod models;
mod processor;
mod schema;
mod utils;

pub mod dao;
pub mod error;
pub mod gnt;
pub mod service;

pub use dummy::DummyDriver;
pub use error::PaymentDriverError;
pub use gnt::GntDriver;
use std::future::Future;
use std::pin::Pin;
use ya_core_model::driver::{
    AccountBalance, AccountMode, Balance, PaymentAmount, PaymentConfirmation, PaymentDetails,
    PaymentStatus,
};

pub type PaymentDriverResult<T> = Result<T, PaymentDriverError>;

pub type SignTx<'a> = &'a (dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>>);

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

pub trait PaymentDriver {
    fn init<'a>(
        &self,
        mode: AccountMode,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'a>>;

    /// Notification when identity gets locked and the driver cannot send transactions
    fn account_locked<'a>(
        &self,
        identity: NodeId,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'a>>;

    /// Notification when identity gets unlocked and the driver can send transactions
    fn account_unlocked<'a>(
        &self,
        identity: NodeId,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'a>>;

    /// Returns account balance
    fn get_account_balance(
        &self,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<AccountBalance>> + 'static>>;

    /// Schedules payment
    fn schedule_payment<'a>(
        &self,
        invoice_id: &str,
        amount: PaymentAmount,
        sender: &str,
        recipient: &str,
        due_date: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'a>>;

    /// Returns payment status
    fn get_payment_status(
        &self,
        invoice_id: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<PaymentStatus>> + 'static>>;

    /// Verifies payment
    fn verify_payment(
        &self,
        confirmation: &PaymentConfirmation,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<PaymentDetails>> + 'static>>;

    /// Returns sum of transactions from payer addr to payee addr
    fn get_transaction_balance(
        &self,
        payer: &str,
        payee: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<Balance>> + 'static>>;
}

#[cfg(feature = "dummy-driver")]
async fn payment_driver_factory(db: &DbExecutor) -> anyhow::Result<impl PaymentDriver> {
    Ok(DummyDriver::new())
}

#[cfg(feature = "gnt-driver")]
async fn payment_driver_factory(db: &DbExecutor) -> anyhow::Result<impl PaymentDriver> {
    Ok(GntDriver::new(db.clone()).await?)
}

pub struct PaymentDriverService;

impl PaymentDriverService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        let db: DbExecutor = context.component();
        let driver = payment_driver_factory(&db).await?;
        let processor = PaymentDriverProcessor::new(driver);
        self::service::bind_service(&db, processor);
        self::service::subscribe_to_identity_events().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {}
