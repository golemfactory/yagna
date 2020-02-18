use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ethereum_types::Address;

mod account;
mod dummy;
mod error;
mod gnt;
mod payment;
pub mod ethereum;

pub use account::{AccountBalance, Balance, Chain, Currency};
pub use dummy::DummyDriver;
pub use error::PaymentDriverError;
pub use gnt::GNTDriver;
pub use payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};

pub type PaymentDriverResult<T> = Result<T, PaymentDriverError>;

#[async_trait]
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
mod tests {
    #[test]
    fn test_it_works() {
        assert!(true);
    }
}
