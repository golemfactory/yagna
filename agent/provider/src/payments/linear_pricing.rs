use anyhow::{anyhow, Result};
use bigdecimal::BigDecimal;
use serde_json::json;

use ya_agreement_utils::ComInfo;
use ya_client_model::payment::Account;

use super::model::{PaymentDescription, PaymentModel};
use crate::market::presets::{Coefficient, Preset};

/// Computes computations costs.
pub struct LinearPricing {
    usage_coeffs: Vec<f64>,
}

impl PaymentModel for LinearPricing {
    fn compute_cost(&self, usage: &Vec<f64>) -> Result<BigDecimal> {
        // Note: last element of usage_coeffs contains constant initial cost
        // of computing task, so we don't multiply it.
        let const_coeff_idx = self.usage_coeffs.len() - 1;
        let cost: f64 = self.usage_coeffs[const_coeff_idx]
            + self.usage_coeffs[0..const_coeff_idx]
                .iter()
                .zip(usage.iter())
                .map(|(coeff, usage_value)| coeff * usage_value)
                .sum::<f64>();
        Ok(BigDecimal::from(cost))
    }

    fn expected_usage_len(&self) -> usize {
        self.usage_coeffs.len() - 1
    }
}

impl LinearPricing {
    pub fn new(commercials: PaymentDescription) -> Result<LinearPricing> {
        let usage: Vec<f64> = commercials.get_usage_coefficients()?;

        log::info!(
            "Creating LinearPricing payment model. Usage coefficients vector: {:?}.",
            usage
        );
        Ok(LinearPricing {
            usage_coeffs: usage,
        })
    }
}

/// Helper for building offer.
pub struct LinearPricingOffer {
    initial_cost: f64,
    usage_coeffs: Vec<f64>,
    usage_params: Vec<String>,
    interval: f64,
}

impl LinearPricingOffer {
    pub fn new() -> LinearPricingOffer {
        // Initialize first constant coefficient to 0.
        LinearPricingOffer {
            usage_coeffs: vec![],
            usage_params: vec![],
            interval: 6.0,
            initial_cost: 0.0,
        }
    }

    pub fn add_coefficient(
        &mut self,
        coefficient: &Coefficient,
        value: f64,
    ) -> Result<&mut LinearPricingOffer> {
        let property = coefficient
            .to_property()
            .ok_or_else(|| anyhow!("Invalid coefficient: {}", coefficient))?
            .to_string();
        self.usage_params.push(property);
        self.usage_coeffs.push(value);
        Ok(self)
    }

    /// Adds constant cost paid no matter how many resources computations will consume.
    pub fn initial_cost(&mut self, value: f64) -> &mut LinearPricingOffer {
        self.initial_cost = value;
        return self;
    }

    pub fn interval(&mut self, seconds: f64) -> &mut LinearPricingOffer {
        self.interval = seconds;
        return self;
    }

    pub fn build(&self, accounts: &Vec<Account>) -> ComInfo {
        let mut coeffs = self.usage_coeffs.clone();
        coeffs.push(self.initial_cost);

        let mut params = json!({
            "scheme": "payu".to_string(),
            "scheme.payu": json!({
                "interval_sec": self.interval
            }),
            "pricing": json!({
                "model": "linear".to_string(),
                "model.linear": json!({
                    "coeffs": coeffs
                })
            }),
            "usage": json!({
                "vector": self.usage_params.clone()
            })
        });

        for account in accounts {
            params.as_object_mut().unwrap().insert(
                format!("payment.platform.{}", account.platform),
                json!({
                    "address".to_string(): account.address,
                }),
            );
        }

        ComInfo { params }
    }

    pub fn from_preset(preset: &Preset) -> Result<LinearPricingOffer> {
        if preset.pricing_model != "linear" {
            return Err(anyhow!(
                "Invalid pricing model [{}] passed to build offer",
                preset.pricing_model
            ));
        }

        let mut pricing_offer = LinearPricingOffer::new();
        for coefficient in Coefficient::variants() {
            if let Some(value) = preset.usage_coeffs.get(&coefficient) {
                match coefficient {
                    Coefficient::Initial => {
                        pricing_offer.initial_cost(*value);
                    }
                    _ => {
                        pricing_offer.add_coefficient(&coefficient, *value)?;
                    }
                }
            }
        }

        Ok(pricing_offer)
    }
}
