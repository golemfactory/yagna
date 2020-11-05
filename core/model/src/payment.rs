use serde::{Deserialize, Serialize};
use ya_client_model::payment::*;
use ya_service_bus::RpcMessage;

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
pub enum RpcMessageError {
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
    use bigdecimal::{BigDecimal, Zero};
    use chrono::{DateTime, Utc};
    use std::fmt::Display;
    use ya_client_model::NodeId;

    pub const BUS_ID: &'static str = "/local/payment";

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
        type Error = GenericError;
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
    #[serde(rename_all = "camelCase")]
    pub struct StatValue {
        pub total_amount: BigDecimal,
        pub agreements_count: u64,
    }

    impl StatValue {
        pub fn new(v: impl Into<BigDecimal>) -> Self {
            let total_amount = v.into();
            let agreements_count = if total_amount.is_zero() { 0 } else { 1 };
            Self {
                total_amount,
                agreements_count,
            }
        }
    }

    impl std::ops::Add for StatValue {
        type Output = Self;

        fn add(self, rhs: Self) -> Self::Output {
            Self {
                agreements_count: self.agreements_count + rhs.agreements_count,
                total_amount: self.total_amount + rhs.total_amount,
            }
        }
    }

    impl std::ops::AddAssign for StatValue {
        fn add_assign(&mut self, rhs: Self) {
            self.agreements_count += rhs.agreements_count;
            self.total_amount += rhs.total_amount;
        }
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Default)]
    pub struct StatusNotes {
        pub requested: StatValue,
        pub accepted: StatValue,
        pub confirmed: StatValue,
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

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct GetAccounts {}

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Account {
        pub platform: String,
        pub address: String,
        pub driver: String,
        pub send: bool,
        pub receive: bool,
    }

    impl RpcMessage for GetAccounts {
        const ID: &'static str = "GetAccounts";
        type Item = Vec<Account>;
        type Error = GenericError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[non_exhaustive]
    pub struct GetInvoiceStats {
        pub node_id: NodeId,
        pub requestor: bool,
        pub provider: bool,
        pub since: DateTime<Utc>,
    }

    impl GetInvoiceStats {
        pub fn new(node_id: NodeId, since: DateTime<Utc>) -> Self {
            Self {
                node_id,
                requestor: true,
                provider: true,
                since,
            }
        }
    }

    impl RpcMessage for GetInvoiceStats {
        const ID: &'static str = "GetInvoiceStats";
        type Item = InvoiceStats;
        type Error = GenericError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Default)]
    #[non_exhaustive]
    pub struct InvoiceStatusNotes {
        pub issued: StatValue,
        pub received: StatValue,
        pub accepted: StatValue,
        pub rejected: StatValue,
        pub failed: StatValue,
        pub settled: StatValue,
        pub cancelled: StatValue,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Default)]
    pub struct InvoiceStats {
        pub requestor: InvoiceStatusNotes,
        pub provider: InvoiceStatusNotes,
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
        pub invoice_id: String,
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
        pub invoice_id: String,
        pub recipient_id: NodeId,
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
