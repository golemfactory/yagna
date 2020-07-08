pub use crate::processor::PaymentDriverProcessor;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use ya_client_model::NodeId;

#[macro_use]
extern crate diesel;

mod ethereum;
mod models;
pub mod processor;
mod schema;
pub mod utils;

pub mod dao;
pub mod error;

pub use error::PaymentDriverError;
use std::future::Future;
use std::pin::Pin;
use ya_core_model::driver::{AccountMode, PaymentConfirmation, PaymentDetails};

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
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<BigDecimal>> + 'static>>;

    /// Schedules payment
    fn schedule_payment<'a>(
        &self,
        amount: BigDecimal,
        sender: &str,
        recipient: &str,
        due_date: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<String>> + 'a>>;


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
    ) -> Pin<Box<dyn Future<Output = PaymentDriverResult<BigDecimal>> + 'static>>;
}

#[cfg(test)]
mod tests {}
