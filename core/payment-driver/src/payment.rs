use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::PaymentEntity;

pub const PAYMENT_STATUS_UNKNOWN: i32 = 0;
pub const PAYMENT_STATUS_NOT_YET: i32 = 1;
pub const PAYMENT_STATUS_OK: i32 = 2;
pub const PAYMENT_STATUS_NOT_ENOUGH_FUNDS: i32 = 3;
pub const PAYMENT_STATUS_NOT_ENOUGH_GAS: i32 = 4;
pub const PAYMENT_STATUS_FAILED: i32 = 5;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentStatus {
    Ok(PaymentConfirmation),
    NotYet,
    NotEnoughFunds,
    NotEnoughGas,
    Unknown,
    Failed,
}

impl PaymentStatus {
    pub fn to_i32(&self) -> i32 {
        match self {
            PaymentStatus::NotYet => PAYMENT_STATUS_NOT_YET,
            PaymentStatus::Ok(_confirmation) => PAYMENT_STATUS_OK,
            PaymentStatus::NotEnoughFunds => PAYMENT_STATUS_NOT_ENOUGH_FUNDS,
            PaymentStatus::NotEnoughGas => PAYMENT_STATUS_NOT_ENOUGH_GAS,
            PaymentStatus::Unknown => PAYMENT_STATUS_UNKNOWN,
            PaymentStatus::Failed => PAYMENT_STATUS_FAILED,
        }
    }
}

impl From<PaymentEntity> for PaymentStatus {
    fn from(payment: PaymentEntity) -> Self {
        match payment.status {
            PAYMENT_STATUS_OK => {
                let confirmation: Vec<u8> = Vec::new();
                PaymentStatus::Ok(PaymentConfirmation {
                    confirmation: confirmation,
                })
            }
            PAYMENT_STATUS_NOT_YET => PaymentStatus::NotYet,
            PAYMENT_STATUS_NOT_ENOUGH_FUNDS => PaymentStatus::NotEnoughFunds,
            PAYMENT_STATUS_NOT_ENOUGH_GAS => PaymentStatus::NotEnoughGas,
            PAYMENT_STATUS_FAILED => PaymentStatus::Failed,
            _ => PaymentStatus::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentDetails {
    pub recipient: String,
    pub sender: String,
    pub amount: BigDecimal,
    pub date: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentAmount {
    pub base_currency_amount: BigDecimal,
    pub gas_amount: Option<BigDecimal>,
}
