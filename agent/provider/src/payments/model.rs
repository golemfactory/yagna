use anyhow::Result;
use bigdecimal::BigDecimal;
use std::time::Duration;

use ya_agreement_utils::{AgreementView, Error};

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

    pub fn get_update_interval(&self) -> Result<Duration> {
        let interval_addr = "/offer/properties/golem/com/scheme/payu/interval_sec";
        let interval = self.agreement.pointer_typed::<f64>(interval_addr)?;
        Ok(Duration::from_secs_f64(interval))
    }

    pub fn get_usage_coefficients(&self) -> Result<Vec<f64>> {
        let coeffs_addr = "/offer/properties/golem/com/pricing/model/linear/coeffs";
        Ok(self.agreement.pointer_typed::<Vec<f64>>(coeffs_addr)?)
    }

    pub fn get_debit_note_deadline(&self) -> Result<Option<chrono::Duration>> {
        match self.agreement.pointer_typed::<i64>(
            "/demand/properties/golem/com/payment/debit-notes/acceptance-timeout",
        ) {
            Ok(deadline) => Ok(Some(chrono::Duration::seconds(deadline))),
            Err(Error::NoKey(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
