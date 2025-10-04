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
    use super::{public::Ack, *};
    use crate::driver::{AccountMode, GasDetails, PaymentConfirmation, ValidateAllocationResult};
    use bigdecimal::{BigDecimal, Zero};
    use chrono::{DateTime, NaiveDateTime, Utc};
    use serde_json::json;
    use std::fmt::Display;
    use std::time::Duration;
    use structopt::*;
    use strum::{EnumProperty, IntoEnumIterator, VariantNames};
    use strum_macros::{Display, EnumIter, EnumString, EnumVariantNames, IntoStaticStr};

    use ya_client_model::{payment::allocation::Deposit, NodeId};

    pub const BUS_SERVICE_NAME: &str = "payment";
    pub const BUS_ID: &str = "/local/payment";
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
    pub enum NoError {} // This is needed because () doesn't implement Display

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
        #[error("Internal timeout")]
        InternalTimeout,
        #[error("Other")]
        Other(String),
    }

    impl RpcMessage for RegisterDriver {
        const ID: &'static str = "RegisterDriver";
        type Item = ();
        type Error = RegisterDriverError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum UnregisterDriverError {
        #[error("Internal timeout")]
        InternalTimeout,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct UnregisterDriver(pub String);

    impl RpcMessage for UnregisterDriver {
        const ID: &'static str = "UnregisterDriver";
        type Item = ();
        type Error = UnregisterDriverError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct RegisterAccount {
        pub address: String,
        pub driver: String,
        pub network: String,
        pub token: String,
        pub mode: AccountMode,
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
        #[error("Internal timeout")]
        InternalTimeout,
    }

    impl RpcMessage for RegisterAccount {
        const ID: &'static str = "RegisterAccount";
        type Item = ();
        type Error = RegisterAccountError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum UnregisterAccountError {
        #[error("Internal timeout")]
        InternalTimeout,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct UnregisterAccount {
        pub platform: String,
        pub address: String,
    }

    impl RpcMessage for UnregisterAccount {
        const ID: &'static str = "UnregisterAccount";
        type Item = ();
        type Error = UnregisterAccountError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct NotifyPayment {
        pub driver: String,
        pub platform: String,
        pub amount: BigDecimal,
        pub sender: String,
        pub recipient: String,
        pub payment_id: String,
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
        pub gas: Option<GasDetails>,
        pub block_number: u64,
        pub block_datetime: DateTime<Utc>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct GetRpcEndpoints {
        pub address: String,
        pub driver: DriverName,
        pub network: Option<NetworkName>,
        pub verify: bool,
        pub resolve: bool,
        pub no_wait: bool,
    }

    impl RpcMessage for crate::payment::local::GetRpcEndpoints {
        const ID: &'static str = "GetRpcEndpoints";
        type Item = crate::payment::local::GetRpcEndpointsResult;
        type Error = GenericError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Default)]
    pub struct GetRpcEndpointsResult {
        pub endpoints: serde_json::Value,
        pub sources: serde_json::Value,
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

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum GetAccountsError {
        #[error("Internal timeout")]
        InternalTimeout,
    }

    impl RpcMessage for GetAccounts {
        const ID: &'static str = "GetAccounts";
        type Item = Vec<Account>;
        type Error = GetAccountsError;
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
        pub timeout: Option<DateTime<Utc>>,
        pub deposit: Option<Deposit>,
        pub new_allocation: bool,
    }

    impl RpcMessage for ValidateAllocation {
        const ID: &'static str = "ValidateAllocation";
        type Item = ValidateAllocationResult;
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
    pub struct BuildPayments {}

    impl RpcMessage for BuildPayments {
        const ID: &'static str = "BuildPayments";
        type Item = String;
        type Error = NoError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ReleaseAllocations {}

    impl RpcMessage for ReleaseAllocations {
        const ID: &'static str = "ReleaseAllocations";
        type Item = ();
        type Error = GenericError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct GetDrivers {
        pub ignore_legacy_networks: bool,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum GetDriversError {
        #[error("Internal timeout")]
        InternalTimeout,
    }

    impl RpcMessage for GetDrivers {
        const ID: &'static str = "GetDrivers";
        type Item = HashMap<String, DriverDetails>;
        type Error = GetDriversError;
    }

    // ********************* STATUS ********************************
    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum PaymentDriverStatusError {
        #[error("Requested driver not registered {0}")]
        NoDriver(String),
        #[error("Requested network not supported {0}")]
        NoNetwork(String),
        #[error("Internal error: {0}")]
        Internal(String),
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct PaymentDriverStatusChange {
        pub properties: Vec<DriverStatusProperty>,
    }

    impl RpcMessage for PaymentDriverStatusChange {
        const ID: &'static str = "PaymentDriverStatusChange";
        type Item = Ack;
        type Error = GenericError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct PaymentDriverStatus {
        pub driver: Option<String>,
        pub network: Option<String>,
    }

    impl RpcMessage for PaymentDriverStatus {
        const ID: &'static str = "PaymentDriverStatus";
        type Item = Vec<DriverStatusProperty>;
        type Error = PaymentDriverStatusError;
    }

    // ********************* GET PROCESS PAYMENTS INFO ********************************
    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum ProcessBatchCycleError {
        #[error("ProcessBatchCycleError: {0}")]
        ProcessBatchCycleError(String),
    }

    impl From<ProcessBatchCycleError> for GenericError {
        fn from(e: ProcessBatchCycleError) -> Self {
            GenericError::new(e)
        }
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ProcessBatchCycleResponse {
        pub node_id: NodeId,
        pub platform: String,
        pub interval: Option<Duration>,
        pub cron: Option<String>,
        pub max_interval: Duration,
        pub extra_payment_time: Duration,
        pub next_process: NaiveDateTime,
        pub last_process: Option<NaiveDateTime>,
    }

    fn round_duration_to_sec(d: Duration) -> Duration {
        //0.500 gives 1.0
        //0.499 gives 0.0
        let secs = ((d.as_millis() + 500) / 1000) as u64;
        Duration::from_secs(secs)
    }

    pub fn batch_cycle_response_to_json(resp: &ProcessBatchCycleResponse) -> serde_json::Value {
        json!({
            "nodeId": resp.node_id,
            "platform": resp.platform,
            "intervalSec": resp.interval.map(|d| d.as_secs()),
            "cron": resp.cron,
            "extraPayTimeSec": round_duration_to_sec(resp.extra_payment_time).as_secs(),
            "maxIntervalSec": round_duration_to_sec(resp.max_interval).as_secs(),
            "nextProcess": resp.next_process.and_utc().to_rfc3339(),
            "lastProcess": resp.last_process.map(|l| l.and_utc().to_rfc3339()),
        })
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ProcessBatchCycleInfo {
        pub node_id: NodeId,
        pub platform: String,
    }

    impl RpcMessage for ProcessBatchCycleInfo {
        const ID: &'static str = "ProcessBatchCycleInfo";
        type Item = ProcessBatchCycleResponse;
        type Error = ProcessBatchCycleError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ProcessBatchCycleSet {
        pub node_id: NodeId,
        pub platform: String,
        pub interval: Option<Duration>,
        pub cron: Option<String>,
        pub next_update: Option<DateTime<Utc>>,
        pub safe_payout: Option<Duration>,
    }

    impl RpcMessage for ProcessBatchCycleSet {
        const ID: &'static str = "ProcessBatchCycleSet";
        type Item = ProcessBatchCycleResponse;
        type Error = ProcessBatchCycleError;
    }

    // ********************* PROCESS PAYMENTS ********************************
    #[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
    pub enum ProcessPaymentsError {
        #[error("ProcessPaymentsError: {0}")]
        ProcessPaymentsError(String),
    }

    impl From<ProcessPaymentsError> for GenericError {
        fn from(e: ProcessPaymentsError) -> Self {
            GenericError::new(e)
        }
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ProcessPaymentsNowResponse {
        pub resolve_time_ms: f64,
        pub send_time_ms: f64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ProcessPaymentsNow {
        pub node_id: NodeId,
        pub platform: String,
        pub skip_resolve: bool,
        pub skip_send: bool,
    }

    impl RpcMessage for ProcessPaymentsNow {
        const ID: &'static str = "ProcessPaymentsNow";
        type Item = ProcessPaymentsNowResponse;
        type Error = ProcessPaymentsError;
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
        EnumIter,
        IntoStaticStr,
        strum_macros::EnumProperty,
        strum_macros::Display,
        Debug,
        Clone,
        PartialEq,
        Eq,
        Ord,
        PartialOrd,
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
        Sepolia,
        #[strum(props(token = "tGLM"))]
        Rinkeby,
        #[strum(props(token = "tGLM"))]
        Goerli,
        #[strum(props(token = "tGLM"))]
        Holesky,
        #[strum(props(token = "tGLM"))]
        Hoodi,
        #[strum(props(token = "GLM"))]
        Polygon,
        #[strum(props(token = "tGLM"))]
        Mumbai,
        #[strum(props(token = "tGLM"))]
        Amoy,
    }

    impl NetworkName {
        pub fn get_token(&self) -> &'static str {
            get_token_from_network_name(self)
        }
    }

    pub fn get_token_from_network_name(network_name: &NetworkName) -> &'static str {
        network_name
            .get_str("token")
            .expect("Strum property token not found")
    }

    impl NetworkName {
        pub fn is_fundable(&self) -> bool {
            use NetworkName::*;
            matches!(self, Sepolia | Goerli | Holesky | Hoodi | Amoy)
        }

        pub fn all_fundable() -> Vec<NetworkName> {
            Self::iter().filter(Self::is_fundable).collect()
        }
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
        Eq,
        Serialize,
        Deserialize,
    )]
    #[strum(serialize_all = "lowercase")]
    #[serde(rename_all = "lowercase")]
    #[non_exhaustive]
    pub enum DriverName {
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
        #[structopt(long, possible_values = NetworkName::VARIANTS, default_value = NetworkName::Hoodi.into())]
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
            assert_eq!("hoodi", a.network());
            assert_eq!("tGLM", a.token());
        }
    }
}

pub mod public {
    use super::*;
    use crate::signable::Signable;
    use ya_client_model::NodeId;

    pub const BUS_ID: &str = "/public/payment";

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

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub struct RejectInvoiceV2 {
        pub invoice_id: String,
        pub rejection: Rejection,
        pub issuer_id: NodeId,
    }

    impl RejectInvoiceV2 {
        pub fn new(invoice_id: String, rejection: Rejection, issuer_id: NodeId) -> Self {
            Self {
                invoice_id,
                rejection,
                issuer_id,
            }
        }
    }

    impl RpcMessage for RejectInvoiceV2 {
        const ID: &'static str = "RejectInvoiceV2";
        type Item = Ack;
        type Error = AcceptRejectError;
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
            Self {
                payment: payment.remove_private_info(),
                signature,
            }
        }
    }

    impl RpcMessage for SendPayment {
        const ID: &'static str = "SendPayment";
        type Item = Ack;
        type Error = SendError;
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct SendSignedPayment {
        #[serde(flatten)]
        pub payment: Payment,
        #[serde(with = "serde_bytes")]
        pub signature: Vec<u8>,
        #[serde(with = "serde_bytes")]
        pub signed_bytes: Vec<u8>,
    }

    impl SendSignedPayment {
        pub fn new(payment: Payment, signature: Vec<u8>) -> Self {
            // Unwrap won't happen, because serialization is always possible.
            let signed_bytes = payment.canonicalize().unwrap_or_default();
            Self {
                payment: payment.remove_private_info(),
                signature,
                signed_bytes,
            }
        }
    }

    impl RpcMessage for SendSignedPayment {
        const ID: &'static str = "SendPaymentWithBytes";
        type Item = Ack;
        type Error = SendError;
    }

    // **************************** SYNC *****************************

    /// Push unsynchronized state
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct PaymentSync {
        /// Payment confirmations.
        pub payments: Vec<SendPayment>,
        /// Invoice acceptances.
        pub invoice_accepts: Vec<AcceptInvoice>,
        /// Invoice rejections.
        pub invoice_rejects: Vec<RejectInvoiceV2>,
        /// Debit note acceptances.
        ///
        /// Only last debit note in chain is included per agreement.
        pub debit_note_accepts: Vec<AcceptDebitNote>,
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct PaymentSyncWithBytes {
        /// Payment confirmations.
        pub payments: Vec<SendSignedPayment>,
        /// Invoice acceptances.
        pub invoice_accepts: Vec<AcceptInvoice>,
        /// Invoice rejections.
        pub invoice_rejects: Vec<RejectInvoiceV2>,
        /// Debit note acceptances.
        ///
        /// Only last debit note in chain is included per agreement.
        pub debit_note_accepts: Vec<AcceptDebitNote>,
    }

    /// Sync error
    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    pub struct PaymentSyncError {
        pub payment_send_errors: Vec<SendError>,
        pub accept_errors: Vec<AcceptRejectError>,
    }

    impl std::fmt::Display for PaymentSyncError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("PaymentSend errors: ")?;
            for send_e in &self.payment_send_errors {
                write!(f, "{}, ", send_e)?;
            }

            f.write_str("Acceptance errors: ")?;
            for accept_e in &self.accept_errors {
                write!(f, "{}, ", accept_e)?;
            }

            Ok(())
        }
    }

    impl std::error::Error for PaymentSyncError {}

    impl RpcMessage for PaymentSync {
        const ID: &'static str = "PaymentSync";
        type Item = Ack;
        type Error = PaymentSyncError;
    }

    impl RpcMessage for PaymentSyncWithBytes {
        const ID: &'static str = "PaymentSyncWithBytes";
        type Item = Ack;
        type Error = PaymentSyncError;
    }

    /// Informs the other side that it should request [`PaymentSync`]
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct PaymentSyncRequest;

    impl RpcMessage for PaymentSyncRequest {
        const ID: &'static str = "PaymentSyncNeeded";
        type Item = Ack;
        type Error = SendError;
    }
}
