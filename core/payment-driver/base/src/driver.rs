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
pub use ya_core_model::payment::local::{Network, Platform};

#[async_trait(?Send)]
pub trait PaymentDriver {
    async fn account_event(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: IdentityEvent,
    ) -> Result<(), IdentityError>;

    async fn get_account_balance(
        &self,
        db: DbExecutor,
        caller: String,
        msg: GetAccountBalance,
    ) -> Result<BigDecimal, GenericError>;

    // used by bus to bind service
    fn get_name(&self) -> String;
    fn get_default_network(&self) -> String;
    fn get_networks(&self) -> HashMap<String, Network>;
    fn recv_init_required(&self) -> bool;

    async fn get_transaction_balance(
        &self,
        db: DbExecutor,
        caller: String,
        msg: GetTransactionBalance,
    ) -> Result<BigDecimal, GenericError>;

    async fn init(&self, db: DbExecutor, caller: String, msg: Init) -> Result<Ack, GenericError>;

    async fn schedule_payment(
        &self,
        db: DbExecutor,
        caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError>;

    async fn verify_payment(
        &self,
        db: DbExecutor,
        caller: String,
        msg: VerifyPayment,
    ) -> Result<PaymentDetails, GenericError>;

    async fn validate_allocation(
        &self,
        db: DbExecutor,
        caller: String,
        msg: ValidateAllocation,
    ) -> Result<bool, GenericError>;
}
