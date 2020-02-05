use serde::{Deserialize, Serialize};
use ya_model::payment::*;
use ya_service_bus::RpcMessage;

pub const SERVICE_ID: &str = "/payment";
pub const BUS_ID: &'static str = "/private/payment";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ack {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SendError {
    ServiceError(String),
    BadRequest(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AcceptRejectError {
    ServiceError(String),
    BadRequest(String),
    ObjectNotFound,
    Forbidden,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CancelError {
    ServiceError(String),
    ObjectNotFound,
    Forbidden,
    Conflict,
}

// ************************** DEBIT NOTE **************************

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SendDebitNote(DebitNote);

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
pub struct SendInvoice(Invoice);

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
