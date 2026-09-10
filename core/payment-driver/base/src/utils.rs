/*
    Common utility functions for dealing with PaymentDriver related objects
*/

// External crates
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use ethereum_types::U256;
use num_bigint::ToBigInt;
use std::fmt::{self, Debug};

// Local uses
use crate::db::models::PaymentEntity;
use crate::model::{PaymentDetails, SchedulePayment};

use ya_client_model::payment::{ActivityPayment, AgreementPayment, Payment};
use ya_core_model::signable::prepare_signature_hash;

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

struct LegacyBigDecimalDebug<'a>(&'a BigDecimal);

impl Debug for LegacyBigDecimalDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // BigDecimal 0.2 represented Debug as BigDecimal("<Display>"), where
        // Display always used plain decimal notation.
        f.debug_tuple("BigDecimal")
            .field(&self.0.to_plain_string())
            .finish()
    }
}

struct LegacyAgreementPaymentDebug<'a>(&'a AgreementPayment);

impl Debug for LegacyAgreementPaymentDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgreementPayment")
            .field("agreement_id", &self.0.agreement_id)
            .field("amount", &LegacyBigDecimalDebug(&self.0.amount))
            .field("allocation_id", &self.0.allocation_id)
            .finish()
    }
}

struct LegacyAgreementPaymentsDebug<'a>(&'a [AgreementPayment]);

impl Debug for LegacyAgreementPaymentsDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(LegacyAgreementPaymentDebug))
            .finish()
    }
}

struct LegacyActivityPaymentDebug<'a>(&'a ActivityPayment);

impl Debug for LegacyActivityPaymentDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActivityPayment")
            .field("activity_id", &self.0.activity_id)
            .field("amount", &LegacyBigDecimalDebug(&self.0.amount))
            .field("allocation_id", &self.0.allocation_id)
            .finish()
    }
}

struct LegacyActivityPaymentsDebug<'a>(&'a [ActivityPayment]);

impl Debug for LegacyActivityPaymentsDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(LegacyActivityPaymentDebug))
            .finish()
    }
}

struct LegacyPaymentDebug<'a>(&'a Payment);

impl Debug for LegacyPaymentDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Payment")
            .field("payment_id", &self.0.payment_id)
            .field("payer_id", &self.0.payer_id)
            .field("payee_id", &self.0.payee_id)
            .field("payer_addr", &self.0.payer_addr)
            .field("payee_addr", &self.0.payee_addr)
            .field("payment_platform", &self.0.payment_platform)
            .field("amount", &LegacyBigDecimalDebug(&self.0.amount))
            .field("timestamp", &self.0.timestamp)
            .field(
                "agreement_payments",
                &LegacyAgreementPaymentsDebug(&self.0.agreement_payments),
            )
            .field(
                "activity_payments",
                &LegacyActivityPaymentsDebug(&self.0.activity_payments),
            )
            .field("details", &self.0.details)
            .finish()
    }
}

fn legacy_payment_preimage(payment: &Payment) -> String {
    format!("{:?}", LegacyPaymentDebug(payment))
}

pub fn payment_hash(payment: &Payment) -> Vec<u8> {
    prepare_signature_hash(legacy_payment_preimage(payment).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::str::FromStr;

    #[test]
    fn legacy_payment_hash_matches_bigdecimal_0_2_vector() {
        let payment = Payment {
            payment_id: "payment-1".into(),
            payer_id: "0x1111111111111111111111111111111111111111"
                .parse()
                .unwrap(),
            payee_id: "0x2222222222222222222222222222222222222222"
                .parse()
                .unwrap(),
            payer_addr: "payer-address".into(),
            payee_addr: "payee-address".into(),
            payment_platform: "erc20-holesky-tglm".into(),
            amount: BigDecimal::from_str("0.00000005").unwrap(),
            timestamp: Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap(),
            agreement_payments: vec![AgreementPayment {
                agreement_id: "agreement-1".into(),
                amount: BigDecimal::from_str("1.2300").unwrap(),
                allocation_id: Some("allocation-a".into()),
            }],
            activity_payments: vec![ActivityPayment {
                activity_id: "activity-1".into(),
                amount: BigDecimal::from_str("0.00000007").unwrap(),
                allocation_id: None,
            }],
            details: r#"{"tx":"0xabc"}"#.into(),
        };
        // Captured from the same Payment shape compiled with BigDecimal 0.2.2.
        let expected_preimage = concat!(
            "Payment { payment_id: \"payment-1\", payer_id: ",
            "0x1111111111111111111111111111111111111111, payee_id: ",
            "0x2222222222222222222222222222222222222222, payer_addr: ",
            "\"payer-address\", payee_addr: \"payee-address\", payment_platform: ",
            "\"erc20-holesky-tglm\", amount: BigDecimal(\"0.00000005\"), timestamp: ",
            "2024-01-02T03:04:05Z, agreement_payments: [AgreementPayment { ",
            "agreement_id: \"agreement-1\", amount: BigDecimal(\"1.2300\"), ",
            "allocation_id: Some(\"allocation-a\") }], activity_payments: ",
            "[ActivityPayment { activity_id: \"activity-1\", amount: ",
            "BigDecimal(\"0.00000007\"), allocation_id: None }], details: ",
            "\"{\\\"tx\\\":\\\"0xabc\\\"}\" }",
        );

        assert_eq!(legacy_payment_preimage(&payment), expected_preimage);
        assert_eq!(
            hex::encode(payment_hash(&payment)),
            "ab1e327b6cc7fe437bffd0a0730f9702fc5de5808eb0aa1690812cb1e3dabec8"
        );
    }
}
