use serde::{Deserialize, Serialize};
use ya_client_model::payment::*;
use ya_service_bus::RpcMessage;

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
pub enum RpcMessageError {
    #[error("Schedule payment error: {0}")]
    Schedule(#[from] local::ScheduleError),
    #[error("Send error: {0}")]
    Send(#[from] public::SendError),
    #[error("Accept/reject error: {0}")]
    AcceptReject(#[from] public::AcceptRejectError),
    #[error("Cancel error: {0}")]
    Cancel(#[from] public::CancelError),
    #[error("{0}")]
    Generic(#[from] local::GenericError),
}

pub mod local {
    use super::*;
    use crate::ethaddr::NodeId;
    use bigdecimal::BigDecimal;
    use std::fmt::Display;

    pub const BUS_ID: &'static str = "/local/payment";

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum ScheduleError {
        #[error("Currency conversion error: {0}")]
        Conversion(String),
        #[error("Invalid address: {0}")]
        Address(String),
        #[error("Payment driver error: {0}")]
        Driver(String),
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct SchedulePayment {
        pub invoice: Invoice,
        pub allocation_id: String,
    }

    impl RpcMessage for SchedulePayment {
        const ID: &'static str = "SchedulePayment";
        type Item = ();
        type Error = ScheduleError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    #[error("{inner}")]
    pub struct GenericError {
        inner: String,
    }

    impl GenericError {
        pub fn new<T: Display>(e: T) -> Self {
            let inner = e.to_string();
            Self { inner }
        }
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Init {
        pub identity: NodeId,
        pub provider: bool,
        pub requestor: bool,
    }

    impl RpcMessage for Init {
        const ID: &'static str = "init";
        type Item = ();
        type Error = GenericError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct GetStatus(NodeId);

    impl From<NodeId> for GetStatus {
        fn from(id: NodeId) -> Self {
            GetStatus(id)
        }
    }

    impl AsRef<NodeId> for GetStatus {
        fn as_ref(&self) -> &NodeId {
            &self.0
        }
    }

    impl GetStatus {
        pub fn identity(&self) -> NodeId {
            self.0
        }
    }

    impl RpcMessage for GetStatus {
        const ID: &'static str = "GetStatus";
        type Item = StatusResult;
        type Error = GenericError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct StatusResult {
        pub amount: BigDecimal,
        pub reserved: BigDecimal,
        pub outgoing: StatusNotes,
        pub incoming: StatusNotes,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, Default)]
    pub struct StatusNotes {
        pub requested: BigDecimal,
        pub accepted: BigDecimal,
        pub confirmed: BigDecimal,
        pub rejected: BigDecimal,
    }

    impl std::ops::Add for StatusNotes {
        type Output = Self;

        fn add(self, rhs: Self) -> Self::Output {
            Self {
                requested: self.requested + rhs.requested,
                accepted: self.accepted + rhs.accepted,
                confirmed: self.confirmed + rhs.confirmed,
                rejected: self.rejected + rhs.rejected,
            }
        }
    }
}

pub mod public {
    use super::*;

    pub const BUS_ID: &'static str = "/public/payment";

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Ack {}

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum SendError {
        #[error("Service error: {0}")]
        ServiceError(String),
        #[error("Bad request: {0}")]
        BadRequest(String),
    }

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum AcceptRejectError {
        #[error("Service error: {0}")]
        ServiceError(String),
        #[error("Bad request: {0}")]
        BadRequest(String),
        #[error("Object not found")]
        ObjectNotFound,
        #[error("Forbidden")]
        Forbidden,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum CancelError {
        #[error("Service error: {0}")]
        ServiceError(String),
        #[error("Object not found")]
        ObjectNotFound,
        #[error("Forbidden")]
        Forbidden,
        #[error("Conflict")]
        Conflict,
    }

    // ************************** DEBIT NOTE **************************
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct SendDebitNote(pub DebitNote);

    impl RpcMessage for SendDebitNote {
        const ID: &'static str = "SendDebitNote";
        type Item = Ack;
        type Error = SendError;
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct AcceptDebitNote {
        pub debit_note_id: String,
        pub acceptance: Acceptance,
    }

    impl RpcMessage for AcceptDebitNote {
        const ID: &'static str = "AcceptDebitNote";
        type Item = Ack;
        type Error = AcceptRejectError;
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RejectDebitNote {
        pub debit_note_id: String,
        pub rejection: Rejection,
    }

    impl RpcMessage for RejectDebitNote {
        const ID: &'static str = "RejectDebitNote";
        type Item = Ack;
        type Error = AcceptRejectError;
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CancelDebitNote {
        pub debit_note_id: String,
    }

    impl RpcMessage for CancelDebitNote {
        const ID: &'static str = "CancelDebitNote";
        type Item = Ack;
        type Error = CancelError;
    }

    // *************************** INVOICE ****************************
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct SendInvoice(pub Invoice);

    impl RpcMessage for SendInvoice {
        const ID: &'static str = "SendInvoice";
        type Item = Ack;
        type Error = SendError;
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct AcceptInvoice {
        pub invoice_id: String,
        pub acceptance: Acceptance,
    }

    impl RpcMessage for AcceptInvoice {
        const ID: &'static str = "AcceptInvoice";
        type Item = Ack;
        type Error = AcceptRejectError;
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RejectInvoice {
        pub debit_note_id: String,
        pub rejection: Rejection,
    }

    impl RpcMessage for RejectInvoice {
        const ID: &'static str = "RejectInvoice";
        type Item = Ack;
        type Error = AcceptRejectError;
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CancelInvoice {
        pub debit_note_id: String,
    }

    impl RpcMessage for CancelInvoice {
        const ID: &'static str = "CancelInvoice";
        type Item = Ack;
        type Error = CancelError;
    }

    // *************************** PAYMENT ****************************
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct SendPayment(pub Payment);

    impl RpcMessage for SendPayment {
        const ID: &'static str = "SendPayment";
        type Item = Ack;
        type Error = SendError;
    }
}
