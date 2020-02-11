use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ethereum_types::{Address, H256};
use web3::{Transport, Web3};
use ya_persistence::executor::DbExecutor;

mod account;
mod payment;
mod payment_driver_error;
pub use account::{AccountBalance, Balance, Chain, Currency};
pub use payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};
pub use payment_driver_error::PaymentDriverError;

#[async_trait]
pub trait PaymentDriver {
    /// Returns account balance
    async fn get_account_balance(&self) -> Result<AccountBalance, PaymentDriverError>;

    /// Schedules payment
    async fn schedule_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        receipent: Address,
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
    async fn get_transcation_balance(&self, payee: Address) -> Result<Balance, PaymentDriverError>;
}

pub struct GNTDriver {}

#[allow(unused)]
impl GNTDriver {
    /// Creates driver from private key
    fn from_private_key<T>(
        private_key: H256,
        web3: Web3<T>,
        chain: Chain,
        db: DbExecutor,
    ) -> GNTDriver
    where
        T: Transport,
    {
        unimplemented!();
    }

    /// Creates driver from keyfile
    fn from_keyfile<T>(
        keyfile: &str,
        password: &str,
        web3: Web3<T>,
        chain: Chain,
        db: DbExecutor,
    ) -> GNTDriver
    where
        T: Transport,
    {
        unimplemented!();
    }
}

#[allow(unused)]
#[async_trait]
impl PaymentDriver for GNTDriver {
    /// Returns account balance
    async fn get_account_balance(&self) -> Result<AccountBalance, PaymentDriverError> {
        unimplemented!();
    }

    /// Schedules payment
    async fn schedule_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        receipent: Address,
        due_date: DateTime<Utc>,
    ) -> Result<(), PaymentDriverError> {
        unimplemented!();
    }

    /// Returns payment status
    async fn get_payment_status(
        &self,
        invoice_id: &str,
    ) -> Result<PaymentStatus, PaymentDriverError> {
        unimplemented!();
    }
    /// Verifies payment
    async fn verify_payment(
        &self,
        confirmation: &PaymentConfirmation,
    ) -> Result<PaymentDetails, PaymentDriverError> {
        unimplemented!();
    }

    /// Returns sum of transactions from given address
    async fn get_transcation_balance(&self, payee: Address) -> Result<Balance, PaymentDriverError> {
        unimplemented!();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_it_works() {
        assert!(true);
    }
}
