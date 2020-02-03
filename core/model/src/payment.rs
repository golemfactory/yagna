use serde::{Deserialize, Serialize};
use ya_model::payment::*;
use ya_service_bus::RpcMessage;

pub const SERVICE_ID: &str = "/payment";
pub const BUS_ID: &'static str = "/private/payment";

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

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
pub enum RpcMessageError {
    #[error("Send error: {0}")]
    Send(#[from] SendError),
    #[error("Accept/reject error: {0}")]
    AcceptReject(#[from] AcceptRejectError),
    #[error("Cancel error: {0}")]
    Cancel(#[from] CancelError),
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
    pub debit_note_id: String,
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
pub struct SendPayment(Payment);

impl RpcMessage for SendPayment {
    const ID: &'static str = "SendPayment";
    type Item = Ack;
    type Error = SendError;
}
