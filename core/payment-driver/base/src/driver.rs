/*
    PaymentDriver is a trait to be implemented by each driver so it can be loaded into the bus.
*/

// External crates
use ethsign::Signature;
use std::collections::HashMap;
use std::convert::TryInto;
use ya_client_model::payment::DriverStatusProperty;

// Workspace uses

// Local uses
use crate::bus;
use crate::model::*;
use crate::utils;

// Public revealed uses, required to implement this trait
pub use async_trait::async_trait;
pub use bigdecimal::BigDecimal;
pub use ya_client_model::payment::network::Network;
pub use ya_client_model::NodeId;
pub use ya_core_model::identity::event::IdentityEvent;
pub use ya_core_model::identity::Error as IdentityError;
use ya_core_model::signable::{prepare_signature_hash, Signable};

#[async_trait(?Send)]
pub trait PaymentDriver {
    async fn account_event(&self, _caller: String, msg: IdentityEvent)
        -> Result<(), IdentityError>;

    async fn get_rpc_endpoints(
        &self,
        caller: String,
        msg: GetRpcEndpoints,
    ) -> Result<GetRpcEndpointsResult, GenericError>;

    async fn get_account_balance(
        &self,
        caller: String,
        msg: GetAccountBalance,
    ) -> Result<GetAccountBalanceResult, GenericError>;

    async fn enter(&self, caller: String, msg: Enter) -> Result<String, GenericError>;

    async fn exit(&self, caller: String, msg: Exit) -> Result<String, GenericError>;

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
    async fn init(&self, caller: String, msg: Init) -> Result<Ack, GenericError>;

    async fn fund(&self, caller: String, msg: Fund) -> Result<String, GenericError>;

    async fn transfer(&self, caller: String, msg: Transfer) -> Result<String, GenericError>;

    async fn schedule_payment(
        &self,
        caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError>;

    async fn verify_payment(
        &self,
        caller: String,
        msg: VerifyPayment,
    ) -> Result<PaymentDetails, GenericError>;

    async fn validate_allocation(
        &self,
        caller: String,
        msg: ValidateAllocation,
    ) -> Result<ValidateAllocationResult, GenericError>;

    async fn release_deposit(
        &self,
        caller: String,
        msg: DriverReleaseDeposit,
    ) -> Result<(), GenericError>;

    async fn sign_payment(
        &self,
        _caller: String,
        msg: SignPayment,
    ) -> Result<Vec<u8>, GenericError> {
        let payment = msg.0.remove_private_info();
        let payload = utils::payment_hash(&payment);
        let node_id = payment.payer_id;
        bus::sign(node_id, payload).await
    }

    async fn sign_payment_canonical(
        &self,
        _caller: String,
        msg: SignPaymentCanonicalized,
    ) -> Result<Vec<u8>, GenericError> {
        let payment = msg.0;
        let payload = payment.hash_canonical().map_err(GenericError::new)?;
        let node_id = payment.payer_id;
        bus::sign(node_id, payload).await
    }

    async fn verify_signature(
        &self,
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

        let payload = if let Some(payload) = msg.canonical {
            match msg.payment.verify_canonical(payload.as_slice()) {
                Ok(_) => prepare_signature_hash(&payload),
                Err(e) => {
                    log::info!(
                        "Signature verification: canonical representation doesn't match struct: {e}"
                    );
                    return Ok(false);
                }
            }
        } else {
            // Backward compatibility version for older Nodes that don't send canonical
            // signed bytes and used Payment debug formatting as representation.
            utils::payment_hash(&msg.payment)
        };
        let pub_key = match signature.recover(payload.as_slice()) {
            Ok(pub_key) => pub_key,
            Err(_) => return Ok(false),
        };

        Ok(pub_key.address() == &msg.payment.payer_id.into_array())
    }

    async fn status(
        &self,
        _caller: String,
        _msg: DriverStatus,
    ) -> Result<Vec<DriverStatusProperty>, DriverStatusError>;

    async fn shut_down(&self, caller: String, msg: ShutDown) -> Result<(), GenericError>;
}
