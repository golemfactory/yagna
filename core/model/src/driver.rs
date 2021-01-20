use bigdecimal::BigDecimal;
use bitflags::bitflags;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use ya_client_model::payment::Allocation;
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
        const NONE = 0b000;
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

// ************************** GET ACCOUNT BALANCE **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetAccountBalance(String);

impl From<String> for GetAccountBalance {
    fn from(address: String) -> Self {
        GetAccountBalance(address)
    }
}

impl GetAccountBalance {
    pub fn address(&self) -> String {
        self.0.clone()
    }
}

impl RpcMessage for GetAccountBalance {
    const ID: &'static str = "GetAccountBalance";
    type Item = BigDecimal;
    type Error = GenericError;
}

// ************************** VERIFY PAYMENT **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifyPayment(PaymentConfirmation);

impl From<PaymentConfirmation> for VerifyPayment {
    fn from(confirmation: PaymentConfirmation) -> Self {
        VerifyPayment(confirmation)
    }
}

impl VerifyPayment {
    pub fn confirmation(&self) -> PaymentConfirmation {
        self.0.clone()
    }
}

impl RpcMessage for VerifyPayment {
    const ID: &'static str = "VerifyPayment";
    type Item = PaymentDetails;
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
    pub fn mode(&self) -> AccountMode {
        self.mode.clone()
    }
}

impl RpcMessage for Init {
    const ID: &'static str = "Init";
    type Item = Ack;
    type Error = GenericError;
}

// ************************** SCHEDULE PAYMENT **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchedulePayment {
    amount: BigDecimal,
    sender: String,
    recipient: String,
    due_date: DateTime<Utc>,
}

impl SchedulePayment {
    pub fn new(
        amount: BigDecimal,
        sender: String,
        recipient: String,
        due_date: DateTime<Utc>,
    ) -> SchedulePayment {
        SchedulePayment {
            amount,
            sender,
            recipient,
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

    pub fn due_date(&self) -> DateTime<Utc> {
        self.due_date.clone()
    }
}

impl RpcMessage for SchedulePayment {
    const ID: &'static str = "SchedulePayment";
    type Item = String; // payment order ID
    type Error = GenericError;
}

// ************************** GET TRANSACTION BALANCE **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetTransactionBalance {
    pub sender: String,
    pub recipient: String,
}

impl GetTransactionBalance {
    pub fn new(sender: String, recipient: String) -> GetTransactionBalance {
        GetTransactionBalance { sender, recipient }
    }
    pub fn sender(&self) -> String {
        self.sender.clone()
    }

    pub fn recipient(&self) -> String {
        self.recipient.clone()
    }
}

impl RpcMessage for GetTransactionBalance {
    const ID: &'static str = "GetTransactionBalance";
    type Item = BigDecimal;
    type Error = GenericError;
}

// ************************** VALIDATE ALLOCATION **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidateAllocation {
    pub address: String,
    pub amount: BigDecimal,
    pub existing_allocations: Vec<Allocation>,
}

impl ValidateAllocation {
    pub fn new(address: String, amount: BigDecimal, existing: Vec<Allocation>) -> Self {
        ValidateAllocation {
            address,
            amount,
            existing_allocations: existing,
        }
    }
}

impl RpcMessage for ValidateAllocation {
    const ID: &'static str = "ValidateAllocation";
    type Item = bool;
    type Error = GenericError;
}

// ************************** ENTER **************************

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Enter {
    amount: BigDecimal,
    network: Option<String>,
    token: Option<String>,
}

impl Enter {
    pub fn new(amount: BigDecimal, network: Option<String>, token: Option<String>) -> Enter {
        Enter {
            amount,
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
    to: Option<String>,
    amount: Option<BigDecimal>,
    network: Option<String>,
    token: Option<String>,
}

impl Exit {
    pub fn new(
        to: Option<String>,
        amount: Option<BigDecimal>,
        network: Option<String>,
        token: Option<String>,
    ) -> Exit {
        Exit {
            to,
            amount,
            network,
            token,
        }
    }

    pub fn amount(&self) -> Option<BigDecimal> {
        match &self.amount {
            Some(a) => Some(a.clone()),
            None => None,
        }
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
    to: String,
    amount: BigDecimal,
    network: Option<String>,
    token: Option<String>,
}

impl Transfer {
    pub fn new(
        to: String,
        amount: BigDecimal,
        network: Option<String>,
        token: Option<String>,
    ) -> Transfer {
        Transfer {
            to,
            amount,
            network,
            token,
        }
    }
}

impl RpcMessage for Transfer {
    const ID: &'static str = "Transfer";
    type Item = String; // Transaction Identifier
    type Error = GenericError;
}
