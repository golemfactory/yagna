use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ethereum_types::Address;

#[macro_use]
extern crate diesel;

mod dummy;

pub mod account;
pub mod dao;
pub mod error;
pub mod ethereum;
pub mod gnt;
pub mod models;
pub mod payment;
pub mod schema;

pub use account::{AccountBalance, Balance, Chain, Currency};
pub use dummy::DummyDriver;
pub use error::PaymentDriverError;
use futures::Future;
pub use gnt::GntDriver;
pub use payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};
use std::pin::Pin;

pub type PaymentDriverResult<T> = Result<T, PaymentDriverError>;

pub type SignTx<'a> = &'a (dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>>);

#[async_trait(?Send)]
pub trait PaymentDriver {
    /// Returns account balance
    async fn get_account_balance(&self) -> PaymentDriverResult<AccountBalance>;

    /// Schedules payment
    async fn schedule_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        recipient: Address,
        due_date: DateTime<Utc>,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<()>;

    /// Returns payment status
    async fn get_payment_status(&self, invoice_id: &str) -> PaymentDriverResult<PaymentStatus>;

    /// Verifies payment
    async fn verify_payment(
        &self,
        confirmation: &PaymentConfirmation,
    ) -> PaymentDriverResult<PaymentDetails>;

    /// Returns sum of transactions from given address
    async fn get_transaction_balance(&self, payee: Address) -> PaymentDriverResult<Balance>;
}

#[cfg(test)]
mod tests {}
