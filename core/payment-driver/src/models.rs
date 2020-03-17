use chrono::NaiveDateTime;

use crate::schema::*;

#[derive(Queryable, Debug, Identifiable, Insertable, PartialEq)]
#[primary_key(tx_hash)]
#[table_name = "gnt_driver_transaction"]
pub struct TransactionEntity {
    pub tx_hash: String,
    pub sender: String,
    pub chain: i32,
    pub nonce: String,
    pub timestamp: NaiveDateTime,
}

#[derive(Queryable, Debug, Identifiable, Insertable, PartialEq)]
#[primary_key(invoice_id)]
#[table_name = "gnt_driver_payment"]
pub struct PaymentEntity {
    pub invoice_id: String,
    pub amount: String,
    pub gas: String,
    pub recipient: String,
    pub payment_due_date: NaiveDateTime,
    pub status: i32,
    pub tx_hash: Option<String>,
}
