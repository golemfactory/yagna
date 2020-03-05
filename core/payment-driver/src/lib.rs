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
use bitflags::bitflags;
pub use dummy::DummyDriver;
pub use error::PaymentDriverError;
use futures::Future;
pub use gnt::GntDriver;
pub use payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};
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

#[async_trait(?Send)]
pub trait PaymentDriver {
    async fn init(
        &self,
        mode: AccountMode,
        address: Address,
        sign_tx: SignTx<'_>,
    ) -> PaymentDriverResult<()>;

    /// Returns account balance
    async fn get_account_balance(&self, address: Address) -> PaymentDriverResult<AccountBalance>;

    /// Schedules payment
    async fn schedule_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        sender: Address,
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

    /// Returns sum of transactions from payer addr to payee addr
    async fn get_transaction_balance(
        &self,
        payer: Address,
        payee: Address,
    ) -> PaymentDriverResult<Balance>;
}

#[cfg(test)]
mod tests {}
