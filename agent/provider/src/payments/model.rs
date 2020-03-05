use anyhow::Result;
use bigdecimal::BigDecimal;
use serde_json::Value;

use ya_model::market::Agreement;


/// Commercial part of agreement.
pub struct PaymentDescription {
    commercial_agreement: Value,
    usage: Vec<f64>,
}

/// Implementation of payment model which knows, how to compute amount
/// of money, that requestor should pay for computations.
pub trait PaymentModel {
    fn compute_cost(self, usage: Vec<f64>) -> Result<BigDecimal>;
}

impl PaymentDescription {
    pub fn new(agreement: &Agreement) -> Result<PaymentDescription> {
        unimplemented!()
    }
}



