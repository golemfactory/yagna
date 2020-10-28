/*
    PaymentDriver is a trait to be implemented by each driver so it can be loaded into the bus.
*/

// External crates

// Workspace uses
use ya_core_model::identity::{event::Event as IdentityEvent, Error as IdentityError};

// Local uses
use crate::account::AccountsRefMut;
use crate::model::{
    Ack, GenericError, GetAccountBalance, GetTransactionBalance, Init, PaymentDetails,
    SchedulePayment, VerifyPayment,
};

// Public revealed uses, required to implement this trait
pub use async_trait::async_trait;
pub use bigdecimal::BigDecimal;
pub use ya_client_model::NodeId;

#[async_trait(?Send)]
pub trait PaymentDriver {
    // -- Required to implement
    async fn get_account_balance(
        &self,
        db: (),
        caller: String,
        msg: GetAccountBalance,
    ) -> Result<BigDecimal, GenericError>;

    // used to update the active accounts, see `accounts.rs` for more details
    fn get_accounts(&self) -> AccountsRefMut;
    // used by bus to bind service
    fn get_name(&self) -> String;
    fn get_platform(&self) -> String;

    async fn get_transaction_balance(
        &self,
        db: (),
        caller: String,
        msg: GetTransactionBalance,
    ) -> Result<BigDecimal, GenericError>;

    async fn init(&self, db: (), caller: String, msg: Init) -> Result<Ack, GenericError>;

    async fn schedule_payment(
        &self,
        db: (),
        caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError>;

    async fn verify_payment(
        &self,
        db: (),
        caller: String,
        msg: VerifyPayment,
    ) -> Result<PaymentDetails, GenericError>;

    // -- Shared functions

    async fn account_event(
        &self,
        _db: (),
        _caller: String,
        msg: IdentityEvent,
    ) -> Result<(), IdentityError> {
        self.get_accounts().handle_event(msg);
        Ok(())
    }
}
