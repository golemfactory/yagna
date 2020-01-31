use chrono::{DateTime, Local};
use ethereum_types::{Address, H256, U256};
use web3::Transport;
use web3::Web3;
use ya_persistence::executor::DbExecutor;

pub enum Chain {
    Mainnet,
    Rinkeby,
}

pub enum PaymentStatus {
    Ok,
    NotYet,
    NotFound,
    NotEnoughFunds,
    NotEnoughGas,
}

#[allow(unused)]
pub struct PaymentDetails {
    receiver: Address,
    amount: U256,
    date: Option<DateTime<Local>>,
}

#[allow(unused)]
pub struct PaymentAmount {
    base_currency_amount: U256,
    gas_amount: Option<U256>,
}

pub trait PaymentDriver {
    /// Creates driver from private key
    fn from_private_key<T: Transport>(
        private_key: H256,
        web3: Web3<T>,
        chain: Chain,
        db: DbExecutor,
    ) -> Self;

    /// Creates driver from keyfile
    fn from_keyfile<T: Transport>(
        keyfile: &str,
        password: &str,
        web3: Web3<T>,
        chain: Chain,
        db: DbExecutor,
    ) -> Self;

    /// Returns account balance
    fn get_account_balance(&self) -> U256;

    /// Schedules payment
    fn schedule_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        receipent: Address,
        due_date: DateTime<Local>,
    ) -> Result<(), &'static str>;

    /// Returns payment status
    fn get_payment_status(&self, invoice_id: &str) -> PaymentStatus;

    /// Verifies payment
    fn verify_payment(&self, payment: PaymentDetails) -> Result<PaymentDetails, &'static str>;

    /// Returns sum of transactions from given address
    fn get_transcation_balance(&self, payee: Address) -> U256;
}

pub struct GNTDriver {}

#[allow(unused)]
impl PaymentDriver for GNTDriver {
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

    /// Returns account balance
    fn get_account_balance(&self) -> U256 {
        unimplemented!();
    }

    /// Schedules payment
    fn schedule_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        receipent: Address,
        due_date: DateTime<Local>,
    ) -> Result<(), &'static str> {
        unimplemented!();
    }

    /// Returns payment status
    fn get_payment_status(&self, invoice_id: &str) -> PaymentStatus {
        unimplemented!();
    }
    /// Verifies payment
    fn verify_payment(&self, payment: PaymentDetails) -> Result<PaymentDetails, &'static str> {
        unimplemented!();
    }

    /// Returns sum of transactions from given address
    fn get_transcation_balance(&self, payee: Address) -> U256 {
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
