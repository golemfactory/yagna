use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ya_client_model::payment::*;
use ya_service_bus::RpcMessage;

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
pub enum RpcMessageError {
    #[error("{0}")]
    Send(#[from] public::SendError),
    #[error("{0}")]
    AcceptReject(#[from] public::AcceptRejectError),
    #[error("{0}")]
    Cancel(#[from] public::CancelError),
    #[error("{0}")]
    Generic(#[from] local::GenericError),
    #[error("{0}")]
    ValidateAllocation(#[from] local::ValidateAllocationError),
}

pub mod local {
    use super::*;
    use crate::driver::{AccountMode, BatchMode, PaymentConfirmation};
    use bigdecimal::{BigDecimal, Zero};
    use chrono::{DateTime, Utc};
    use std::fmt::Display;
    use std::time::Duration;
    use structopt::*;
    use strum::{EnumProperty, VariantNames};
    use strum_macros::{Display, EnumProperty, EnumString, EnumVariantNames, IntoStaticStr};

    use ya_client_model::NodeId;

    pub const BUS_ID: &'static str = "/local/payment";
    pub const DEFAULT_PAYMENT_DRIVER: &str = "erc20";

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
        pub title: Option<PaymentTitle>,
        pub payer_id: NodeId,
        pub payee_id: NodeId,
        pub payer_addr: String,
        pub payee_addr: String,
        pub payment_platform: String,
        pub allocation_id: Option<String>,
        pub amount: BigDecimal,
        pub due_date: DateTime<Utc>,
    }

    impl SchedulePayment {
        pub fn from_order(
            payer_id: NodeId,
            payee_id: NodeId,
            payer_addr: String,
            payee_addr: String,
            payment_platform: String,
            amount: BigDecimal,
        ) -> Self {
            SchedulePayment {
                title: None,
                payer_id,
                payee_id,
                payer_addr,
                payee_addr,
                payment_platform,
                allocation_id: None,
                amount,
                due_date: Utc::now(),
            }
        }

        pub fn from_invoice(
            invoice: Invoice,
            allocation_id: String,
            amount: BigDecimal,
        ) -> Option<Self> {
            if amount <= BigDecimal::zero() {
                return None;
            }
            Some(Self {
                title: Some(PaymentTitle::Invoice(InvoicePayment {
                    invoice_id: invoice.invoice_id,
                    agreement_id: invoice.agreement_id,
                })),
                payer_id: invoice.recipient_id,
                payee_id: invoice.issuer_id,
                payer_addr: invoice.payer_addr,
                payee_addr: invoice.payee_addr,
                payment_platform: invoice.payment_platform,
                allocation_id: Some(allocation_id),
                amount,
                due_date: invoice.payment_due_date,
            })
        }

        pub fn from_debit_note(
            debit_note: DebitNote,
            allocation_id: String,
            amount: BigDecimal,
        ) -> Option<Self> {
            if amount <= BigDecimal::zero() {
                return None;
            }
            debit_note.payment_due_date.map(|due_date| Self {
                title: Some(PaymentTitle::DebitNote(DebitNotePayment {
                    debit_note_id: debit_note.debit_note_id,
                    activity_id: debit_note.activity_id,
                })),
                payer_id: debit_note.recipient_id,
                payee_id: debit_note.issuer_id,
                payer_addr: debit_note.payer_addr,
                payee_addr: debit_note.payee_addr,
                payment_platform: debit_note.payment_platform,
                allocation_id: Some(allocation_id),
                amount,
                due_date,
            })
        }

        pub fn document_id(&self) -> String {
            match &self.title {
                Some(PaymentTitle::Invoice(invoice_payment)) => invoice_payment.invoice_id.clone(),
                Some(PaymentTitle::DebitNote(debit_note_payment)) => {
                    debit_note_payment.debit_note_id.clone()
                }
                None => Default::default(),
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

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    #[error("")]
    pub struct NoError {} // This is needed because () doesn't implement Display

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct RegisterDriver {
        pub driver_name: String,
        pub details: DriverDetails,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum RegisterDriverError {
        #[error("Invalid default token specified: token={0}, network={1}")]
        InvalidDefaultToken(String, String),
        #[error("Invalid default network specified: {0}")]
        InvalidDefaultNetwork(String),
    }

    impl RpcMessage for RegisterDriver {
        const ID: &'static str = "RegisterDriver";
        type Item = ();
        type Error = RegisterDriverError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct UnregisterDriver(pub String);

    impl RpcMessage for UnregisterDriver {
        const ID: &'static str = "UnregisterDriver";
        type Item = ();
        type Error = NoError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct RegisterAccount {
        pub address: String,
        pub driver: String,
        pub network: String,
        pub token: String,
        pub mode: AccountMode,
        pub batch: Option<BatchMode>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum RegisterAccountError {
        #[error("Account already registered: address={0}, driver={1}")]
        AlreadyRegistered(String, String),
        #[error("Driver not registered: {0}")]
        DriverNotRegistered(String),
        #[error("Network not supported by driver: network={0}, driver={1}")]
        UnsupportedNetwork(String, String),
        #[error("Token not supported by driver: token={0}, network={1}, driver={2}")]
        UnsupportedToken(String, String, String),
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

    impl RpcMessage for UnregisterAccount {
        const ID: &'static str = "UnregisterAccount";
        type Item = ();
        type Error = NoError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct NotifyPayment {
        pub driver: String,
        pub platform: String,
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
        pub address: String,
        pub driver: String,
        pub network: Option<String>,
        pub token: Option<String>,
        pub after_timestamp: i64,
    }

    impl RpcMessage for GetStatus {
        const ID: &'static str = "GetStatus";
        type Item = StatusResult;
        type Error = GenericError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Default)]
    pub struct StatusResult {
        pub amount: BigDecimal,
        pub reserved: BigDecimal,
        pub outgoing: StatusNotes,
        pub incoming: StatusNotes,
        pub driver: String,
        pub network: String,
        pub token: String,
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
        pub overdue: Option<StatValue>,
    }

    impl std::ops::Add for StatusNotes {
        type Output = Self;

        fn add(self, rhs: Self) -> Self::Output {
            Self {
                requested: self.requested + rhs.requested,
                accepted: self.accepted + rhs.accepted,
                confirmed: self.confirmed + rhs.confirmed,
                overdue: match (self.overdue, rhs.overdue) {
                    (None, None) => None,
                    (Some(l), Some(r)) => Some(l + r),
                    (Some(l), None) => Some(l),
                    (None, Some(r)) => Some(r),
                },
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

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ValidateAllocation {
        pub platform: String,
        pub address: String,
        pub amount: BigDecimal,
    }

    impl RpcMessage for ValidateAllocation {
        const ID: &'static str = "ValidateAllocation";
        type Item = bool;
        type Error = ValidateAllocationError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum ValidateAllocationError {
        #[error("Account not registered")]
        AccountNotRegistered,
        #[error("Error while validating allocation: {0}")]
        Other(String),
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ReleaseAllocations {}

    impl RpcMessage for ReleaseAllocations {
        const ID: &'static str = "ReleaseAllocations";
        type Item = ();
        type Error = GenericError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct BuildPayments {}

    impl RpcMessage for BuildPayments {
        const ID: &'static str = "BuildPayments";
        type Item = String;
        type Error = NoError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct GetDrivers {}

    impl RpcMessage for GetDrivers {
        const ID: &'static str = "GetDrivers";
        type Item = HashMap<String, DriverDetails>;
        type Error = NoError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ShutDown {
        pub timeout: Duration,
    }

    impl ShutDown {
        pub fn new(timeout: Duration) -> Self {
            Self { timeout }
        }
    }

    impl RpcMessage for ShutDown {
        const ID: &'static str = "ShutDown";
        type Item = ();
        type Error = GenericError;
    }

    /// Experimental. In future releases this might change or be removed.
    #[derive(
        EnumString,
        EnumVariantNames,
        IntoStaticStr,
        EnumProperty,
        Display,
        Debug,
        Clone,
        PartialEq,
        Serialize,
        Deserialize,
    )]
    #[strum(serialize_all = "lowercase")]
    #[serde(rename_all = "lowercase")]
    #[non_exhaustive]
    pub enum NetworkName {
        #[strum(props(token = "GLM"))]
        Mainnet,
        #[strum(props(token = "tGLM"))]
        Rinkeby,
        #[strum(props(token = "tGLM"))]
        Goerli,
        #[strum(props(token = "GLM"))]
        Polygon,
        #[strum(props(token = "tGLM"))]
        Mumbai,
    }

    /// Experimental. In future releases this might change or be removed.
    #[derive(
        EnumString,
        EnumVariantNames,
        IntoStaticStr,
        Display,
        Debug,
        Clone,
        PartialEq,
        Serialize,
        Deserialize,
    )]
    #[strum(serialize_all = "lowercase")]
    #[serde(rename_all = "lowercase")]
    #[non_exhaustive]
    pub enum DriverName {
        ZkSync,
        Erc20,
    }

    #[derive(StructOpt, Debug, Clone)]
    pub struct AccountCli {
        /// Wallet address [default: <DEFAULT_IDENTITY>]
        #[structopt(long, env = "YA_ACCOUNT")]
        pub account: Option<NodeId>,
        /// Payment driver
        #[structopt(long, possible_values = DriverName::VARIANTS, default_value = DriverName::Erc20.into())]
        pub driver: DriverName,
        /// Payment network
        #[structopt(long, possible_values = NetworkName::VARIANTS, default_value = NetworkName::Rinkeby.into())]
        pub network: NetworkName,
    }

    impl AccountCli {
        pub fn address(&self) -> Option<String> {
            self.account.map(|a| a.to_string())
        }

        pub fn driver(&self) -> String {
            self.driver.to_string()
        }

        pub fn network(&self) -> String {
            self.network.to_string()
        }

        pub fn token(&self) -> String {
            self.network.get_str("token").unwrap().to_string()
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[test]
        fn test_cli_defaults() {
            let a = AccountCli::from_iter(&[""]);
            assert_eq!(None, a.address());
            assert_eq!("erc20", a.driver());
            assert_eq!("rinkeby", a.network());
            assert_eq!("tGLM", a.token());
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
    pub struct SendPayment {
        #[serde(flatten)]
        pub payment: Payment,
        pub signature: Vec<u8>,
    }

    impl SendPayment {
        pub fn new(payment: Payment, signature: Vec<u8>) -> Self {
            Self { payment, signature }
        }
    }

    impl RpcMessage for SendPayment {
        const ID: &'static str = "SendPayment";
        type Item = Ack;
        type Error = SendError;
    }
}
