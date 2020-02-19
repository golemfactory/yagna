use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ethereum_types::Address;

mod account;
mod dummy;
mod error;
mod gnt;
mod payment;

pub use account::{AccountBalance, Balance, Chain, Currency};
pub use dummy::DummyDriver;
pub use error::PaymentDriverError;
pub use gnt::GNTDriver;
pub use payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};

#[async_trait]
pub trait PaymentDriver {
    /// Returns account balance
    async fn get_account_balance(&self) -> Result<AccountBalance, PaymentDriverError>;

    /// Schedules payment
    async fn schedule_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        recipient: Address,
        due_date: DateTime<Utc>,
    ) -> Result<(), PaymentDriverError>;

    /// Returns payment status
    async fn get_payment_status(
        &self,
        invoice_id: &str,
    ) -> Result<PaymentStatus, PaymentDriverError>;

    /// Verifies payment
    async fn verify_payment(
        &self,
        confirmation: &PaymentConfirmation,
    ) -> Result<PaymentDetails, PaymentDriverError>;

    /// Returns sum of transactions from given address
    async fn get_transaction_balance(&self, payee: Address) -> Result<Balance, PaymentDriverError>;
}
