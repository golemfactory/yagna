use chrono::{DateTime, Utc};
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::Provider;

#[macro_use]
extern crate diesel;

mod dummy;
mod ethereum;
mod models;
mod schema;
mod utils;

pub mod account;
pub mod dao;
pub mod error;
pub mod gnt;
pub mod payment;

pub use account::{AccountBalance, Balance, Currency};
use bitflags::bitflags;
pub use dummy::DummyDriver;
pub use error::PaymentDriverError;
pub use gnt::GntDriver;
pub use payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};
use std::future::Future;
use std::pin::Pin;

pub type PaymentDriverResult<T> = Result<T, PaymentDriverError>;

pub type SignTx<'a> = &'a (dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>>);

bitflags! {
    pub struct AccountMode : usize {
        const NONE = 0b000;
        const RECV = 0b001;
        const SEND = 0b010;
        const ALL = Self::RECV.bits | Self::SEND.bits;
    }
}

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

pub trait PaymentDriver {
    fn init<'a>(
        &self,
        mode: AccountMode,
        address: &str,
        sign_tx: SignTx<'a>,
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
        sign_tx: SignTx<'a>,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'a>>;

    /// Schedules payment
    fn reschedule_payment<'a>(
        &self,
        invoice_id: &str,
        sign_tx: SignTx<'a>,
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
        let _driver = payment_driver_factory(&db).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {}
