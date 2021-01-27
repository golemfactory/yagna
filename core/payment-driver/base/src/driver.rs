/*
    PaymentDriver is a trait to be implemented by each driver so it can be loaded into the bus.
*/

// External crates

// Workspace uses

// Local uses
use crate::dao::DbExecutor;
use crate::model::*;

// Public revealed uses, required to implement this trait
pub use async_trait::async_trait;
pub use bigdecimal::BigDecimal;
use std::collections::HashMap;
pub use ya_client_model::NodeId;
pub use ya_core_model::identity::{event::Event as IdentityEvent, Error as IdentityError};
pub use ya_core_model::payment::local::Network;

#[async_trait(?Send)]
pub trait PaymentDriver {
    /// Called by the Identity service to notify the driver that specified
    /// account is _locked_ / _unlocked_. Identity service holds
    /// accounts private keys and signs transactions.
    async fn account_event(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: IdentityEvent,
    ) -> Result<(), IdentityError>;

    /// Gets the balance of the account.
    async fn get_account_balance(
        &self,
        db: DbExecutor,
        caller: String,
        msg: GetAccountBalance,
    ) -> Result<BigDecimal, GenericError>;

    /// Deposits the funds into the driver's supported network. Called by CLI.
    async fn enter(
        &self,
        db: DbExecutor,
        caller: String,
        msg: Enter,
    ) -> Result<String, GenericError>;

    /// Exits the funds outside the driver's supported network (most likely L1).
    /// Called by CLI.
    async fn exit(&self, db: DbExecutor, caller: String, msg: Exit)
        -> Result<String, GenericError>;

    // used by bus to bind service
    fn get_name(&self) -> String;
    fn get_default_network(&self) -> String;
    fn get_networks(&self) -> HashMap<String, Network>;

    /// Tells whether account initialization is needed for receiving payments.
    fn recv_init_required(&self) -> bool;

    /// NOTE: DEPRECATED. Drivers should return very big number (e.g. `1_000_000_000_000_000_000u64` or the whole token supply)
    /// Gets the balance of the funds sent from the sender to the recipient.
    async fn get_transaction_balance(
        &self,
        db: DbExecutor,
        caller: String,
        msg: GetTransactionBalance,
    ) -> Result<BigDecimal, GenericError>;

    /// Initializes the account to be used with the driver service. It should call
    /// `bus::register_account` to notify Payment service about the driver readiness. Driver can handle multiple accounts.
    async fn init(&self, db: DbExecutor, caller: String, msg: Init) -> Result<Ack, GenericError>;

    /// Funds the account from faucet when run on testnet. Provides instructions how to fund on mainnet.
    async fn fund(&self, db: DbExecutor, caller: String, msg: Fund)
        -> Result<String, GenericError>;

    /// Transfers the funds between specified accounts. Called by CLI.
    async fn transfer(
        &self,
        db: DbExecutor,
        caller: String,
        msg: Transfer,
    ) -> Result<String, GenericError>;

    /// Schedules the payment between specified accounts. Payments
    /// are processed by the `cron` job. Payment tracking is done by
    /// cron job, see: `PaymentDriverCron::confirm_payments`.
    async fn schedule_payment(
        &self,
        db: DbExecutor,
        caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError>;

    /// Verifies the payment transaction by transaction's confirmation (transaction's identifier).
    async fn verify_payment(
        &self,
        db: DbExecutor,
        caller: String,
        msg: VerifyPayment,
    ) -> Result<PaymentDetails, GenericError>;

    /// Validates that allocated funds are still sufficient to cover
    /// the costs of the task (including the transaction fees, e.g. Ethereum's Gas).
    /// Allocation is created when the requestor publishes the task on the market.
    async fn validate_allocation(
        &self,
        db: DbExecutor,
        caller: String,
        msg: ValidateAllocation,
    ) -> Result<bool, GenericError>;
}
