use crate::{
    AccountBalance, Balance, Chain, PaymentAmount, PaymentConfirmation, PaymentDetails,
    PaymentDriver, PaymentDriverError, PaymentStatus,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ethereum_types::{Address, H256};
use web3::{Transport, Web3};
use ya_persistence::executor::DbExecutor;

pub struct GNTDriver {}

#[allow(unused)]
impl GNTDriver {
    /// Creates driver from private key
    pub fn from_private_key<T>(
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
    pub fn from_keyfile<T>(
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

#[async_trait]
#[allow(unused)]
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
        recipient: Address,
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
    async fn get_transaction_balance(&self, payer: Address) -> Result<Balance, PaymentDriverError> {
        unimplemented!();
    }
}
