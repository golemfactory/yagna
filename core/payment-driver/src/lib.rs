use chrono::{DateTime, Utc};

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
        &'a mut self,
        mode: AccountMode,
        address: &str,
        sign_tx: SignTx<'_>,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'static>>;

    /// Returns account balance
    fn get_account_balance<'a>(
        &'a self,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<AccountBalance>> + 'static>>;

    /// Schedules payment
    fn schedule_payment<'a>(
        &'a mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        sender: &str,
        recipient: &str,
        due_date: DateTime<Utc>,
        sign_tx: SignTx<'_>,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'static>>;

    /// Schedules payment
    fn reschedule_payment<'a>(
        &'a mut self,
        invoice_id: &str,
        sign_tx: SignTx<'_>,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<()>> + 'static>>;

    /// Returns payment status
    fn get_payment_status<'a>(
        &'a self,
        invoice_id: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<PaymentStatus>> + 'static>>;

    /// Verifies payment
    fn verify_payment<'a>(
        &'a self,
        confirmation: &PaymentConfirmation,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<PaymentDetails>> + 'static>>;

    /// Returns sum of transactions from payer addr to payee addr
    fn get_transaction_balance<'a>(
        &'a self,
        payer: &str,
        payee: &str,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<Balance>> + 'static>>;
}

#[cfg(test)]
mod tests {}
