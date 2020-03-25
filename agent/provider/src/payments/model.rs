use anyhow::{anyhow, Result};
use bigdecimal::BigDecimal;
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
    pub commercial_agreement: HashMap<String, String>,
}

impl PaymentDescription {
    pub fn new(agreement: &Agreement) -> Result<PaymentDescription> {
        let properties = &agreement.offer.properties;

        let commercial = properties
            .as_object()
            .ok_or(anyhow!("Agreement properties has unexpected format."))?
            .iter()
            .filter(|(key, _)| key.starts_with("golem.com."))
            .filter(|(_, value)| value.is_string())
            .map(|(key, value)| (key.clone(), value.as_str().unwrap().to_string()))
            .collect::<HashMap<String, String>>();

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
        let interval = interval.parse::<u64>().map_err(|error| {
            anyhow!(
                "Can't parse payment update interval '{}' to u64. {}",
                error,
                interval_addr
            )
        })?;
        Ok(Duration::from_secs(interval))
    }

    pub fn get_usage_coefficients(&self) -> Result<Vec<f64>> {
        let coeffs_addr = "golem.com.pricing.model.linear.coeffs";
        let usage_vec_str = self.commercial_agreement.get(coeffs_addr).ok_or(anyhow!(
            "Can't find usage coefficients in agreement ('{}').",
            coeffs_addr
        ))?;

        let usage: Vec<f64> = serde_json::from_str(&usage_vec_str)
            .map_err(|error| anyhow!("Can't parse usage vector. Error: {}", error))?;
        Ok(usage)
    }
}
