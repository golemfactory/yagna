/*
    PaymentDriver is a trait to be implemented by each driver so it can be loaded into the bus.
*/

// External crates
use ethsign::Signature;
use std::collections::HashMap;
use std::convert::TryInto;

// Workspace uses

// Local uses
use crate::bus;
use crate::dao::DbExecutor;
use crate::model::*;
use crate::utils;

// Public revealed uses, required to implement this trait
pub use async_trait::async_trait;
pub use bigdecimal::BigDecimal;
pub use ya_client_model::payment::network::Network;
pub use ya_client_model::NodeId;
pub use ya_core_model::identity::{event::Event as IdentityEvent, Error as IdentityError};

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

    async fn get_account_gas_balance(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: GetAccountGasBalance,
    ) -> Result<Option<GasDetails>, GenericError>;

    async fn enter(
        &self,
        db: DbExecutor,
        caller: String,
        msg: Enter,
    ) -> Result<String, GenericError>;

    async fn exit(&self, db: DbExecutor, caller: String, msg: Exit)
        -> Result<String, GenericError>;

    // used by bus to bind service
    fn get_name(&self) -> String;
    fn get_default_network(&self) -> String;
    fn get_networks(&self) -> HashMap<String, Network>;
    fn recv_init_required(&self) -> bool;

    /// There is no guarentee that this method will be called only once
    /// AccountMode in Init message should be incremental i.e. :
    ///     first init with mode: Send
    ///     second init with mode: Recv
    ///     should result in driver capable of both Sending & Receiving
    async fn init(&self, db: DbExecutor, caller: String, msg: Init) -> Result<Ack, GenericError>;

    async fn fund(&self, db: DbExecutor, caller: String, msg: Fund)
        -> Result<String, GenericError>;

    async fn transfer(
        &self,
        db: DbExecutor,
        caller: String,
        msg: Transfer,
    ) -> Result<String, GenericError>;

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

    async fn sign_payment(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: SignPayment,
    ) -> Result<Vec<u8>, GenericError> {
        let payload = utils::payment_hash(&msg.0);
        let node_id = msg.0.payer_id;
        bus::sign(node_id, payload).await
    }

    async fn verify_signature(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: VerifySignature,
    ) -> Result<bool, GenericError> {
        if msg.signature.len() != 65 {
            return Ok(false);
        }
        let v = msg.signature[0];
        let r: [u8; 32] = msg.signature[1..33].try_into().unwrap();
        let s: [u8; 32] = msg.signature[33..65].try_into().unwrap();
        let signature = Signature { v, r, s };

        let payload = utils::payment_hash(&msg.payment);
        let pub_key = match signature.recover(payload.as_slice()) {
            Ok(pub_key) => pub_key,
            Err(_) => return Ok(false),
        };

        Ok(pub_key.address() == &msg.payment.payer_id.into_array())
    }

    async fn shut_down(
        &self,
        db: DbExecutor,
        caller: String,
        msg: ShutDown,
    ) -> Result<(), GenericError>;
}
