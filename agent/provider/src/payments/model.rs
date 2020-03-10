use anyhow::{Result, anyhow};
use bigdecimal::BigDecimal;
use serde_json::Value;

use ya_model::market::Agreement;


/// Commercial part of agreement.
pub struct PaymentDescription {
    pub commercial_agreement: Value,
    pub usage_coeffs: Vec<f64>,
}

/// Implementation of payment model which knows, how to compute amount
/// of money, that requestor should pay for computations.
pub trait PaymentModel {
    fn compute_cost(&self, usage: &Vec<f64>) -> Result<BigDecimal>;
}

impl PaymentDescription {
    pub fn new(agreement: &Agreement) -> Result<PaymentDescription> {
        let properties = &agreement.offer.properties;
        log::debug!("{}", properties);

        let commercial = properties.pointer("golem.com")
            .ok_or(anyhow!("Can't find commercial part of agreement ('golem.com')."))?;

        let usage_vec_str = properties.pointer("golem.com.usage.vector")
            .ok_or(anyhow!("Can't find usage vector in agreement ('golem.com.usage.vector')."))?
            .as_str()
            .ok_or(anyhow!("Usage vector from agreement is not a string ('golem.com.usage.vector')."))?;

        let usage: Vec<f64> = serde_json::from_str(usage_vec_str)
            .map_err(|error|anyhow!("Can't parse usage vector."))?;

        Ok(PaymentDescription{commercial_agreement: commercial.clone(), usage_coeffs: usage})
    }
}