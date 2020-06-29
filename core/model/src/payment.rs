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
    use crate::driver::{AccountMode, PaymentConfirmation};
    use bigdecimal::BigDecimal;
    use chrono::{DateTime, Utc};
    use std::fmt::Display;
    use ya_client_model::NodeId;

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
    pub struct DebitNotePayment {
        pub debit_note_id: String,
        pub activity_id: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct InvoicePayment {
        pub invoice_id: String,
        pub agreement_id: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub enum PaymentTitle {
        DebitNote(DebitNotePayment),
        Invoice(InvoicePayment),
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct SchedulePayment {
        pub title: PaymentTitle,
        pub payer_id: NodeId,
        pub payee_id: NodeId,
        pub payer_addr: String,
        pub payee_addr: String,
        pub payment_platform: String,
        pub allocation_id: String,
        pub amount: BigDecimal,
        pub due_date: DateTime<Utc>,
    }

    impl SchedulePayment {
        pub fn from_invoice(invoice: Invoice, allocation_id: String, amount: BigDecimal) -> Self {
            Self {
                title: PaymentTitle::Invoice(InvoicePayment {
                    invoice_id: invoice.invoice_id,
                    agreement_id: invoice.agreement_id,
                }),
                payer_id: invoice.recipient_id,
                payee_id: invoice.issuer_id,
                payer_addr: invoice.payer_addr,
                payee_addr: invoice.payee_addr,
                payment_platform: invoice.payment_platform,
                allocation_id,
                amount,
                due_date: invoice.payment_due_date,
            }
        }

        pub fn from_debit_note(
            debit_note: DebitNote,
            allocation_id: String,
            amount: BigDecimal,
        ) -> Option<Self> {
            debit_note.payment_due_date.map(|due_date| Self {
                title: PaymentTitle::DebitNote(DebitNotePayment {
                    debit_note_id: debit_note.debit_note_id,
                    activity_id: debit_note.activity_id,
                }),
                payer_id: debit_note.recipient_id,
                payee_id: debit_note.issuer_id,
                payer_addr: debit_note.payer_addr,
                payee_addr: debit_note.payee_addr,
                payment_platform: debit_note.payment_platform,
                allocation_id,
                amount,
                due_date,
            })
        }

        pub fn document_id(&self) -> String {
            match &self.title {
                PaymentTitle::Invoice(invoice_payment) => invoice_payment.invoice_id.clone(),
                PaymentTitle::DebitNote(debit_note_payment) => {
                    debit_note_payment.debit_note_id.clone()
                }
            }
        }
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
    pub struct RegisterAccount {
        pub platform: String,
        pub address: String,
        pub driver: String,
        pub mode: AccountMode,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum RegisterAccountError {
        #[error("Account already registered")]
        AlreadyRegistered,
        #[error("Error while registering account: {0}")]
        Other(String),
    }

    impl RpcMessage for RegisterAccount {
        const ID: &'static str = "RegisterAccount";
        type Item = ();
        type Error = RegisterAccountError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct UnregisterAccount {
        pub platform: String,
        pub address: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum UnregisterAccountError {
        #[error("Account not registered")]
        NotRegistered,
        #[error("Error while unregistering account: {0}")]
        Other(String),
    }

    impl RpcMessage for UnregisterAccount {
        const ID: &'static str = "UnregisterAccount";
        type Item = ();
        type Error = UnregisterAccountError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct NotifyPayment {
        pub driver: String,
        pub amount: BigDecimal,
        pub sender: String,
        pub recipient: String,
        pub order_ids: Vec<String>,
        pub confirmation: PaymentConfirmation,
    }

    impl RpcMessage for NotifyPayment {
        const ID: &'static str = "NotifyPayment";
        type Item = ();
        type Error = GenericError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct GetStatus {
        pub platform: String,
        pub address: String,
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
    }

    impl std::ops::Add for StatusNotes {
        type Output = Self;

        fn add(self, rhs: Self) -> Self::Output {
            Self {
                requested: self.requested + rhs.requested,
                accepted: self.accepted + rhs.accepted,
                confirmed: self.confirmed + rhs.confirmed,
            }
        }
    }

    impl std::iter::Sum<StatusNotes> for StatusNotes {
        fn sum<I: Iterator<Item = StatusNotes>>(iter: I) -> Self {
            iter.fold(Default::default(), |acc, item| acc + item)
        }
    }
}

pub mod public {
    use super::*;
    use ya_client_model::NodeId;

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
        pub issuer_id: NodeId,
    }

    impl AcceptDebitNote {
        pub fn new(debit_note_id: String, acceptance: Acceptance, issuer_id: NodeId) -> Self {
            Self {
                debit_note_id,
                acceptance,
                issuer_id,
            }
        }
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
        pub issuer_id: NodeId,
    }

    impl AcceptInvoice {
        pub fn new(invoice_id: String, acceptance: Acceptance, issuer_id: NodeId) -> Self {
            Self {
                invoice_id,
                acceptance,
                issuer_id,
            }
        }
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
