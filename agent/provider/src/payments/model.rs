use anyhow::{anyhow, Result};
use bigdecimal::BigDecimal;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

use ya_model::market::Agreement;

/// Implementation of payment model which knows, how to compute amount
/// of money, that requestor should pay for computations.
pub trait PaymentModel {
    fn compute_cost(&self, usage: &Vec<f64>) -> Result<BigDecimal>;
    fn expected_usage_len(&self) -> usize;
}

/// Extracted commercial part of agreement.
pub struct PaymentDescription {
    pub commercial_agreement: HashMap<String, Value>,
}

impl PaymentDescription {
    pub fn new(agreement: &Agreement) -> Result<PaymentDescription> {
        let properties = &agreement.offer.properties;

        let commercial = properties
            .as_object()
            .ok_or(anyhow!("Agreement properties has unexpected format."))?
            .iter()
            .filter(|(key, _)| key.starts_with("golem.com."))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<HashMap<String, Value>>();

        log::debug!("Commercial properties:\n{:#?}", &commercial);

        Ok(PaymentDescription {
            commercial_agreement: commercial.clone(),
        })
    }

    pub fn get_update_interval(&self) -> Result<Duration> {
        let interval_addr = "golem.com.scheme.payu.interval_sec";
        let interval = self.commercial_agreement.get(interval_addr).ok_or(anyhow!(
            "Can't get payment update interval '{}'.",
            interval_addr
        ))?;
        let interval = interval.as_f64().ok_or(anyhow!(
            "Can't parse payment update interval '{}' to u64.",
            interval_addr
        ))?;
        Ok(Duration::from_secs_f64(interval))
    }

    pub fn get_usage_coefficients(&self) -> Result<Vec<f64>> {
        let coeffs_addr = "golem.com.pricing.model.linear.coeffs";
        let usage_vec = self
            .commercial_agreement
            .get(coeffs_addr)
            .ok_or(anyhow!(
                "Can't find usage coefficients in agreement ('{}').",
                coeffs_addr
            ))?
            .clone();

        let usage: Vec<f64> = serde_json::from_value(usage_vec).map_err(|error| {
            anyhow!(
                "Can't parse usage vector '{}'. Error: {}",
                coeffs_addr,
                error
            )
        })?;

        Ok(usage)
    }
}
