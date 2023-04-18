/*
    Common utility functions for dealing with PaymentDriver related objects
*/

// External crates
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use ethereum_types::U256;
use num_bigint::ToBigInt;
use sha3::{Digest, Sha3_256};

// Local uses
use crate::db::models::PaymentEntity;
use crate::model::{PaymentDetails, SchedulePayment};
use ya_client_model::payment::Payment;

const PRECISION: u64 = 1_000_000_000_000_000_000;

pub fn msg_to_payment_details(
    msg: &SchedulePayment,
    date: Option<DateTime<Utc>>,
) -> PaymentDetails {
    PaymentDetails {
        recipient: msg.recipient(),
        sender: msg.sender(),
        amount: msg.amount(),
        date,
    }
}

pub fn db_to_payment_details(payment: &PaymentEntity) -> PaymentDetails {
    // TODO: Put date in database?
    let date = Utc::now();
    PaymentDetails {
        recipient: payment.recipient.clone(),
        sender: payment.sender.clone(),
        amount: db_amount_to_big_dec(payment.amount.clone()),
        date: Some(date),
    }
}

pub fn db_amount_to_big_dec(amount: String) -> BigDecimal {
    u256_to_big_dec(u256_from_big_endian_hex(amount))
}

pub fn u256_to_big_endian_hex(value: U256) -> String {
    let mut bytes = [0u8; 32];
    value.to_big_endian(&mut bytes);
    hex::encode(bytes)
}

pub fn u256_from_big_endian_hex(bytes: String) -> U256 {
    let bytes = hex::decode(bytes).unwrap();
    U256::from_big_endian(&bytes)
}

pub fn big_dec_to_u256(v: &BigDecimal) -> U256 {
    let v = v * Into::<BigDecimal>::into(PRECISION);
    let v = v.to_bigint().unwrap();
    let v = &v.to_string();
    U256::from_dec_str(v).unwrap()
}

pub fn u256_to_big_dec(v: U256) -> BigDecimal {
    let v: BigDecimal = v.to_string().parse().unwrap();
    v / Into::<BigDecimal>::into(PRECISION)
}

pub fn payment_hash(payment: &Payment) -> Vec<u8> {
    let mut hasher = Sha3_256::new();
    hasher.update(format!("{:?}", payment).as_bytes());
    hasher.finalize().to_vec()
}
