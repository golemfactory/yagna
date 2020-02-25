use chrono::NaiveDateTime;

use crate::schema::*;

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[primary_key(tx_hash)]
#[table_name = "gnt_driver_transaction"]
pub struct TransactionEntity {
    tx_hash: String,
    sender: String,
    chain: i32,
    nonce: String,
    timestamp: NaiveDateTime,
}

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[primary_key(invoice_id)]
#[table_name = "gnt_driver_payment"]
pub struct PaymentEntity {
    invoice_id: String,
    amount: String,
    gas: String,
    recipient: String,
    payment_due_date: NaiveDateTime,
    status: i32,
    tx_hash: Option<String>,
}
