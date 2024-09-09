use bigdecimal::BigDecimal;
use bitflags::bitflags;
use chrono::{DateTime, Utc};
use derive_more::From;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::time::Duration;
use ya_client_model::payment::{Allocation, DriverStatusProperty, Payment};
use ya_client_model::NodeId;
use ya_service_bus::RpcMessage;

pub fn driver_bus_id<T: Display>(driver_name: T) -> String {
    format!("/local/driver/{}", driver_name)
}

// ************************** ERROR **************************

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

// ************************** ACK **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ack {}

// ************************** ACCOUNT **************************

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct AccountMode : usize {
        const RECV = 0b001;
        const SEND = 0b010;
        const ALL = Self::RECV.bits | Self::SEND.bits;
    }
}

// ************************** PAYMENT **************************

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentDetails {
    pub recipient: String,
    pub sender: String,
    pub amount: BigDecimal,
    pub date: Option<DateTime<Utc>>,
}

impl Display for PaymentDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "({{recipient: {}, sender: {}, amount: {}, date: {}}})",
            self.recipient,
            self.sender,
            self.amount,
            self.date.unwrap_or_default()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentConfirmation {
    pub confirmation: Vec<u8>,
}

impl PaymentConfirmation {
    pub fn from(bytes: &[u8]) -> PaymentConfirmation {
        PaymentConfirmation {
            confirmation: bytes.to_vec(),
        }
    }
}

// ************************** GET RPC ENDPOINTS INFO **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetRpcEndpoints {
    pub network: Option<String>,
    pub verify: bool,
    pub resolve: bool,
    pub no_wait: bool,
}

impl RpcMessage for GetRpcEndpoints {
    const ID: &'static str = "GetRpcEndpoints";
    type Item = GetRpcEndpointsResult;
    type Error = GenericError;
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct GetRpcEndpointsResult {
    pub endpoints: serde_json::Value,
    pub sources: serde_json::Value,
}

// ************************** GET ACCOUNT BALANCE **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetAccountBalance {
    address: String,
    platform: String,
}

impl GetAccountBalance {
    pub fn new(address: String, platform: String) -> Self {
        GetAccountBalance { address, platform }
    }
}

impl GetAccountBalance {
    pub fn address(&self) -> String {
        self.address.clone()
    }
    pub fn platform(&self) -> String {
        self.platform.clone()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GetAccountBalanceResult {
    pub gas_details: Option<GasDetails>,
    pub token_balance: BigDecimal,
    pub block_number: u64,
    pub block_datetime: DateTime<Utc>,
}

impl RpcMessage for GetAccountBalance {
    const ID: &'static str = "GetAccountBalance";
    type Item = GetAccountBalanceResult;
    type Error = GenericError;
}

// ************************** GET TRANSACTION BALANCE **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetTransactionBalance {
    pub sender: String,
    pub recipient: String,
    pub platform: String,
}

impl GetTransactionBalance {
    pub fn new(sender: String, recipient: String, platform: String) -> GetTransactionBalance {
        GetTransactionBalance {
            sender,
            recipient,
            platform,
        }
    }
    pub fn sender(&self) -> String {
        self.sender.clone()
    }

    pub fn recipient(&self) -> String {
        self.recipient.clone()
    }

    pub fn platform(&self) -> String {
        self.platform.clone()
    }
}

impl RpcMessage for GetTransactionBalance {
    const ID: &'static str = "GetTransactionBalance";
    type Item = BigDecimal;
    type Error = GenericError;
}

// ************************** VERIFY PAYMENT **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifyPayment {
    pub confirmation: PaymentConfirmation,
    pub platform: String,
    pub details: Payment,
}

impl VerifyPayment {
    pub fn new(confirmation: PaymentConfirmation, platform: String, details: Payment) -> Self {
        Self {
            confirmation,
            platform,
            details,
        }
    }
}

impl VerifyPayment {
    pub fn confirmation(&self) -> PaymentConfirmation {
        self.confirmation.clone()
    }
    pub fn platform(&self) -> String {
        self.platform.clone()
    }
}

impl RpcMessage for VerifyPayment {
    const ID: &'static str = "VerifyPayment";
    type Item = PaymentDetails;
    type Error = GenericError;
}

// ************************** FUND **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fund {
    address: String,
    network: Option<String>,
    token: Option<String>,
    mint_only: bool,
}

impl Fund {
    pub fn new(
        address: String,
        network: Option<String>,
        token: Option<String>,
        mint_only: bool,
    ) -> Self {
        Self {
            address,
            network,
            token,
            mint_only,
        }
    }
    pub fn address(&self) -> String {
        self.address.clone()
    }
    pub fn network(&self) -> Option<String> {
        self.network.clone()
    }
    pub fn token(&self) -> Option<String> {
        self.token.clone()
    }
    pub fn mint_only(&self) -> bool {
        self.mint_only
    }
}

impl RpcMessage for Fund {
    const ID: &'static str = "Fund";
    type Item = String;
    type Error = GenericError;
}

// ************************** INIT **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Init {
    address: String,
    network: Option<String>,
    token: Option<String>,
    mode: AccountMode,
}

impl Init {
    pub fn new(
        address: String,
        network: Option<String>,
        token: Option<String>,
        mode: AccountMode,
    ) -> Init {
        Init {
            address,
            network,
            token,
            mode,
        }
    }
    pub fn address(&self) -> String {
        self.address.clone()
    }
    pub fn network(&self) -> Option<String> {
        self.network.clone()
    }
    pub fn token(&self) -> Option<String> {
        self.token.clone()
    }
    pub fn mode(&self) -> AccountMode {
        self.mode
    }
}

impl RpcMessage for Init {
    const ID: &'static str = "Init";
    type Item = Ack;
    type Error = GenericError;
}

/*
// ************************** TRY UPDATE PAYMENT **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TryUpdatePaymentResult {
    PaymentNotFound,
    PaymentUpdated,
    PaymentNotUpdated,
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TryUpdatePayment {
    payment_id: String,
    amount: BigDecimal,
    sender: String,
    recipient: String,
    platform: String,
    deposit_id: Option<Deposit>,
    due_date: DateTime<Utc>,
}

impl TryUpdatePayment {
    pub fn new(
        payment_id: String,
        amount: BigDecimal,
        sender: String,
        recipient: String,
        platform: String,
        deposit_id: Option<Deposit>,
        due_date: DateTime<Utc>,
    ) -> TryUpdatePayment {
        TryUpdatePayment {
            payment_id,
            amount,
            sender,
            recipient,
            platform,
            deposit_id,
            due_date,
        }
    }

    pub fn payment_id(&self) -> String {
        self.payment_id.clone()
    }

    pub fn amount(&self) -> BigDecimal {
        self.amount.clone()
    }

    pub fn sender(&self) -> String {
        self.sender.clone()
    }

    pub fn recipient(&self) -> String {
        self.recipient.clone()
    }

    pub fn platform(&self) -> String {
        self.platform.clone()
    }

    pub fn deposit_id(&self) -> Option<Deposit> {
        self.deposit_id.clone()
    }

    pub fn due_date(&self) -> DateTime<Utc> {
        self.due_date
    }
}

impl RpcMessage for TryUpdatePayment {
    const ID: &'static str = "TryUpdatePayment";
    type Item = TryUpdatePaymentResult;
    type Error = GenericError;
}
*/
// ************************** FLUSH PAYMENTS **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlushPayments {
    node_id: Option<NodeId>,
    flush_date: DateTime<Utc>,
}

impl FlushPayments {
    pub fn new(node_id: Option<NodeId>, flush_date: DateTime<Utc>) -> FlushPayments {
        FlushPayments {
            node_id,
            flush_date,
        }
    }

    pub fn flush_date(&self) -> DateTime<Utc> {
        self.flush_date
    }

    pub fn node_id(&self) -> Option<NodeId> {
        self.node_id
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FlushPaymentResult {
    FlushScheduled,
    FlushNotNeeded,
}

impl RpcMessage for FlushPayments {
    const ID: &'static str = "FlushPayments";
    type Item = FlushPaymentResult;
    type Error = GenericError;
}

// ************************** SCHEDULE PAYMENT **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduleDriverPayment {
    amount: BigDecimal,
    sender: String,
    recipient: String,
    platform: String,
    deposit_id: Option<Deposit>,
    due_date: DateTime<Utc>,
}

impl ScheduleDriverPayment {
    pub fn new(
        amount: BigDecimal,
        sender: String,
        recipient: String,
        platform: String,
        deposit_id: Option<Deposit>,
        due_date: DateTime<Utc>,
    ) -> ScheduleDriverPayment {
        ScheduleDriverPayment {
            amount,
            sender,
            recipient,
            platform,
            deposit_id,
            due_date,
        }
    }

    pub fn amount(&self) -> BigDecimal {
        self.amount.clone()
    }

    pub fn sender(&self) -> String {
        self.sender.clone()
    }

    pub fn recipient(&self) -> String {
        self.recipient.clone()
    }

    pub fn platform(&self) -> String {
        self.platform.clone()
    }

    pub fn deposit_id(&self) -> Option<Deposit> {
        self.deposit_id.clone()
    }

    pub fn due_date(&self) -> DateTime<Utc> {
        self.due_date
    }
}

impl RpcMessage for ScheduleDriverPayment {
    const ID: &'static str = "ScheduleDriverPayment";
    type Item = String; // payment order ID
    type Error = GenericError;
}

// ************************** VALIDATE ALLOCATION **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidateAllocation {
    pub address: String,
    pub platform: String,
    pub amount: BigDecimal,
    pub timeout: Option<DateTime<Utc>>,
    pub deposit: Option<Deposit>,
    pub past_allocations: Vec<Allocation>,
    pub active_allocations: Vec<Allocation>,
    pub new_allocation: bool,
}

impl ValidateAllocation {
    pub fn new(
        address: String,
        platform: String,
        amount: BigDecimal,
        timeout: Option<DateTime<Utc>>,
        past_allocations: Vec<Allocation>,
        active_allocations: Vec<Allocation>,
        new_allocation: bool,
    ) -> Self {
        ValidateAllocation {
            address,
            platform,
            amount,
            timeout,
            deposit: None,
            past_allocations,
            active_allocations,
            new_allocation,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ValidateAllocationResult {
    InsufficientAccountFunds {
        requested_funds: BigDecimal,
        available_funds: BigDecimal,
        reserved_funds: BigDecimal,
    },
    InsufficientDepositFunds {
        requested_funds: BigDecimal,
        available_funds: BigDecimal,
    },
    TimeoutExceedsDeposit {
        requested_timeout: Option<DateTime<Utc>>,
        deposit_timeout: DateTime<Utc>,
    },
    TimeoutPassed {
        requested_timeout: DateTime<Utc>,
    },
    MalformedDepositContract,
    MalformedDepositId,
    NoDeposit {
        deposit_id: String,
    },
    DepositReused {
        allocation_id: String,
    },
    DepositSpenderMismatch {
        deposit_spender: String,
    },
    DepositValidationError(String),
    Valid,
}

impl RpcMessage for ValidateAllocation {
    const ID: &'static str = "ValidateAllocation";
    type Item = ValidateAllocationResult;
    type Error = GenericError;
}

// ************************** ENTER **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Enter {
    pub amount: BigDecimal,
    pub address: String,
    pub network: Option<String>,
    pub token: Option<String>,
}

impl Enter {
    pub fn new(
        amount: BigDecimal,
        address: String,
        network: Option<String>,
        token: Option<String>,
    ) -> Enter {
        Enter {
            amount,
            address,
            network,
            token,
        }
    }
}

impl RpcMessage for Enter {
    const ID: &'static str = "Enter";
    type Item = String; // Transaction Identifier
    type Error = GenericError;
}

// ************************** EXIT **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Exit {
    sender: String,
    to: Option<String>,
    amount: Option<BigDecimal>,
    network: Option<String>,
    token: Option<String>,
}

impl Exit {
    pub fn new(
        sender: String,
        to: Option<String>,
        amount: Option<BigDecimal>,
        network: Option<String>,
        token: Option<String>,
    ) -> Exit {
        Exit {
            sender,
            to,
            amount,
            network,
            token,
        }
    }

    pub fn amount(&self) -> Option<BigDecimal> {
        self.amount.clone()
    }
    pub fn sender(&self) -> String {
        self.sender.clone()
    }
    pub fn to(&self) -> Option<String> {
        self.to.clone()
    }
    pub fn network(&self) -> Option<String> {
        self.network.clone()
    }
}

impl RpcMessage for Exit {
    const ID: &'static str = "Exit";
    type Item = String; // Transaction Identifier
    type Error = GenericError;
}

// ************************** TRANSFER **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transfer {
    pub sender: String,
    pub to: String,
    pub amount: BigDecimal,
    pub network: Option<String>,
    pub token: Option<String>,
    pub gas_price: Option<BigDecimal>,
    pub max_gas_price: Option<BigDecimal>,
    pub gas_limit: Option<u32>,
    pub gasless: bool,
}

#[allow(clippy::too_many_arguments)]
impl Transfer {
    pub fn new(
        sender: String,
        to: String,
        amount: BigDecimal,
        network: Option<String>,
        token: Option<String>,
        gas_price: Option<BigDecimal>,
        max_gas_price: Option<BigDecimal>,
        gas_limit: Option<u32>,
        gasless: bool,
    ) -> Transfer {
        Transfer {
            sender,
            to,
            amount,
            network,
            token,
            gas_price,
            max_gas_price,
            gas_limit,
            gasless,
        }
    }
}

impl RpcMessage for Transfer {
    const ID: &'static str = "Transfer";
    type Item = String; // Transaction Identifier
    type Error = GenericError;
}

// ************************ SIGN PAYMENT ************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignPayment(pub Payment);

impl From<Payment> for SignPayment {
    fn from(payment: Payment) -> Self {
        Self(payment)
    }
}

impl RpcMessage for SignPayment {
    const ID: &'static str = "SignPayment";
    type Item = Vec<u8>;
    type Error = GenericError;
}

// ************************ SIGN PAYMENT ************************

/// We sign canonicalized version of `Payment` struct, so although we could make new struct compatible in terms of deserialization, the signature would be incorrect. That's why we need separate endpoint.
#[derive(Clone, Debug, Serialize, Deserialize, From)]
pub struct SignPaymentCanonicalized(pub Payment);

impl RpcMessage for crate::driver::SignPaymentCanonicalized {
    const ID: &'static str = "SignPaymentCanonicalized";
    type Item = Vec<u8>;
    type Error = GenericError;
}

// ********************** VERIFY SIGNATURE **********************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifySignature {
    pub payment: Payment,
    pub signature: Vec<u8>,
    pub canonical: Option<Vec<u8>>,
}

impl VerifySignature {
    pub fn new(payment: Payment, signature: Vec<u8>, canonical: Option<Vec<u8>>) -> Self {
        Self {
            payment,
            signature,
            canonical,
        }
    }
}

impl RpcMessage for VerifySignature {
    const ID: &'static str = "VerifySignature";
    type Item = bool; // is signature correct
    type Error = GenericError;
}

// ********************* STATUS ********************************
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriverStatus {
    pub network: Option<String>,
}

impl RpcMessage for DriverStatus {
    const ID: &'static str = "DriverStatus";
    type Item = Vec<DriverStatusProperty>;
    type Error = DriverStatusError;
}

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
pub enum DriverStatusError {
    #[error("No such network '{0}'")]
    NetworkNotFound(String),
}

// ************************* DEPOSIT *************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriverReleaseDeposit {
    pub platform: String,
    pub from: String,
    pub deposit_contract: String,
    pub deposit_id: String,
}

impl RpcMessage for DriverReleaseDeposit {
    const ID: &'static str = "DriverReleaseDeposit";
    type Item = ();
    type Error = GenericError;
}

// ************************* SHUT DOWN *************************

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

// ************************* GAS DETAILS *************************

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct GasDetails {
    pub currency_short_name: String,
    pub currency_long_name: String,
    pub balance: BigDecimal,
}
