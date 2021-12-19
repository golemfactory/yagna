use anyhow::Result;
use bigdecimal::BigDecimal;
use std::time::Duration;

use ya_agreement_utils::{AgreementView, Error};

use crate::market::negotiator::builtin::expiration::DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY;
use crate::market::negotiator::builtin::note_interval::{
    DEBIT_NOTE_INTERVAL_PROPERTY, DEFAULT_DEBIT_NOTE_INTERVAL_SEC,
};
use crate::market::negotiator::builtin::payment_timeout::PAYMENT_TIMEOUT_PROPERTY;

/// Implementation of payment model which knows, how to compute amount
/// of money, that requestor should pay for computations.
pub trait PaymentModel {
    fn compute_cost(&self, usage: &Vec<f64>) -> Result<BigDecimal>;
    fn expected_usage_len(&self) -> usize;
}

/// Extracted commercial part of agreement.
pub struct PaymentDescription<'a> {
    pub agreement: &'a AgreementView,
}

impl<'a> PaymentDescription<'a> {
    pub fn new(agreement: &'a AgreementView) -> Result<PaymentDescription<'a>> {
        Ok(PaymentDescription::<'a> { agreement })
    }

    pub fn get_usage_coefficients(&self) -> Result<Vec<f64>> {
        let coeffs_addr = "/offer/properties/golem/com/pricing/model/linear/coeffs";
        Ok(self.agreement.pointer_typed::<Vec<f64>>(coeffs_addr)?)
    }

    pub fn get_update_interval(&self) -> Result<Duration> {
        let interval = match self.agreement.pointer_typed::<u32>(&format!(
            "/offer/properties{}",
            DEBIT_NOTE_INTERVAL_PROPERTY
        )) {
            Ok(interval) => interval,
            Err(Error::NoKey(_)) => DEFAULT_DEBIT_NOTE_INTERVAL_SEC,
            Err(error) => return Err(error.into()),
        };
        Ok(Duration::from_secs(interval as u64))
    }

    pub fn get_payment_timeout(&self) -> Result<Option<chrono::Duration>> {
        match self
            .agreement
            .pointer_typed::<u32>(&format!("/offer/properties{}", PAYMENT_TIMEOUT_PROPERTY))
        {
            Ok(dur) => Ok(Some(chrono::Duration::seconds(dur as i64))),
            Err(Error::NoKey(_)) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub fn get_debit_note_accept_timeout(&self) -> Result<Option<chrono::Duration>> {
        match self.agreement.pointer_typed::<u32>(&format!(
            "/offer/properties{}",
            DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY
        )) {
            Ok(dur) => Ok(Some(chrono::Duration::seconds(dur as i64))),
            Err(Error::NoKey(_)) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}
