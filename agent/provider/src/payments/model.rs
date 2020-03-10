use anyhow::{Result, anyhow};
use bigdecimal::BigDecimal;
use serde_json::Value;
use std::collections::HashMap;

use ya_model::market::Agreement;


/// Commercial part of agreement.
pub struct PaymentDescription {
    pub commercial_agreement: HashMap<String, String>,
}

/// Implementation of payment model which knows, how to compute amount
/// of money, that requestor should pay for computations.
pub trait PaymentModel {
    fn compute_cost(&self, usage: &Vec<f64>) -> Result<BigDecimal>;
}

impl PaymentDescription {
    pub fn new(agreement: &Agreement) -> Result<PaymentDescription> {
        let properties = &agreement.offer.properties;
        log::info!("{}", serde_json::to_string_pretty(&properties)?);

        let commercial = properties.as_object()
            .ok_or(anyhow!("Agreement properties has unexpected format."))?
            .iter()
            .filter(|(key, _)| { key.starts_with("golem.com.") })
            .filter(|(_, value)| { value.is_string() })
            .map(|(key, value)| {
                (key.clone(), value.as_str().unwrap().to_string())
            }).collect::<HashMap<String, String>>();

        Ok(PaymentDescription{commercial_agreement: commercial.clone()})
    }
}