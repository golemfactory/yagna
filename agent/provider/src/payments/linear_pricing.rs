use anyhow::Result;
use bigdecimal::BigDecimal;
use serde_json::json;

use ya_agent_offer_model::ComInfo;

use super::model::{PaymentDescription, PaymentModel};

/// Computes computations costs.
pub struct LinearPricing {
    usage_coeffs: Vec<f64>,
}

impl PaymentModel for LinearPricing {
    fn compute_cost(&self, usage: &Vec<f64>) -> Result<BigDecimal> {
        // Note: first element of usage_coeffs contains constant initial cost
        // of computing task, so we don't multiply it.
        let cost: f64 = self.usage_coeffs[0]
            + self.usage_coeffs[1..]
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
    usage_coeffs: Vec<f64>,
    usage_params: Vec<String>,
    interval: f64,
}

impl LinearPricingOffer {
    pub fn new() -> LinearPricingOffer {
        // Initialize first constant coefficient to 0.
        LinearPricingOffer {
            usage_coeffs: vec![0.0],
            usage_params: vec![],
            interval: 6.0,
        }
    }

    pub fn add_coefficient(&mut self, coeff_name: &str, value: f64) -> &mut LinearPricingOffer {
        self.usage_params.push(coeff_name.to_string());
        self.usage_coeffs.push(value);
        return self;
    }

    /// Adds constant cost paid no matter how many resources computations will consume.
    pub fn initial_cost(&mut self, value: f64) -> &mut LinearPricingOffer {
        self.usage_coeffs[0] = value;
        return self;
    }

    pub fn interval(&mut self, seconds: f64) -> &mut LinearPricingOffer {
        self.interval = seconds;
        return self;
    }

    pub fn build(&self) -> ComInfo {
        let params = json!({
            "scheme": "payu".to_string(),
            "scheme.payu": json!({
                "interval_sec": self.interval
            }),
            "pricing": json!({
                "model": "linear".to_string(),
                "model.linear": json!({
                    "coeffs": self.usage_coeffs.clone()
                })
            }),
            "usage": json!({
                "vector": self.usage_params.clone()
            })
        });

        ComInfo { params }
    }
}
