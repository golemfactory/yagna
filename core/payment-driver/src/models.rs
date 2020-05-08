use chrono::NaiveDateTime;

use crate::schema::*;

const TX_CREATED: i32 = 1;
const TX_SENT: i32 = 2;
const TX_CONFIRMED: i32 = 3;
const TX_FAILED: i32 = 0;

const TRANSFER_TX: i32 = 1;
const FAUCET_TX: i32 = 0;

#[derive(Clone, Copy)]
pub enum TxType {
    Transfer,
    Faucet,
}

impl Into<i32> for TxType {
    fn into(self) -> i32 {
        match &self {
            TxType::Transfer => TRANSFER_TX,
            TxType::Faucet => FAUCET_TX,
        }
    }
}

pub enum TransactionStatus {
    Created,
    Sent,
    Confirmed,
    Failed,
}

impl From<i32> for TransactionStatus {
    fn from(status: i32) -> Self {
        match status {
            TX_CREATED => TransactionStatus::Created,
            TX_SENT => TransactionStatus::Sent,
            TX_CONFIRMED => TransactionStatus::Confirmed,
            TX_FAILED => TransactionStatus::Failed,
            _ => panic!("Unknown tx status"),
        }
    }
}

impl Into<i32> for TransactionStatus {
    fn into(self) -> i32 {
        match &self {
            TransactionStatus::Created => TX_CREATED,
            TransactionStatus::Sent => TX_SENT,
            TransactionStatus::Confirmed => TX_CONFIRMED,
            TransactionStatus::Failed => TX_FAILED,
        }
    }
}

#[derive(Clone, Queryable, Debug, Identifiable, Insertable, PartialEq)]
#[primary_key(tx_hash)]
#[table_name = "gnt_driver_transaction"]
pub struct TransactionEntity {
    pub tx_id: String,
    pub sender: String,
    pub nonce: String,
    pub timestamp: NaiveDateTime,
    pub status: i32,
    pub tx_type: i32,
    pub encoded: String,
    pub signature: String,
    pub tx_hash: Option<String>,
}

#[derive(Queryable, Clone, Debug, Identifiable, Insertable, PartialEq)]
#[primary_key(invoice_id)]
#[table_name = "gnt_driver_payment"]
pub struct PaymentEntity {
    pub invoice_id: String,
    pub amount: String,
    pub gas: String,
    pub sender: String,
    pub recipient: String,
    pub payment_due_date: NaiveDateTime,
    pub status: i32,
    pub tx_id: Option<String>,
}
