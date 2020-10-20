use anyhow::Result;
use bigdecimal::BigDecimal;
use serde_json::json;

use ya_agreement_utils::ComInfo;
use ya_client_model::payment::Account;

use super::model::{PaymentDescription, PaymentModel};
use crate::market::presets::{Coefficient, Preset};

pub trait PricingOffer {
    fn prices(&self, preset: &Preset) -> Vec<(Coefficient, f64)>;
    fn build(
        &self,
        accounts: &Vec<Account>,
        initial_price: f64,
        prices: Vec<(String, f64)>,
    ) -> Result<ComInfo>;
}

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
    interval: f64,
}

impl Default for LinearPricingOffer {
    fn default() -> Self {
        LinearPricingOffer { interval: 6.0 }
    }
}

impl LinearPricingOffer {
    #[allow(unused)]
    pub fn interval(mut self, seconds: f64) -> Self {
        self.interval = seconds;
        return self;
    }
}

impl PricingOffer for LinearPricingOffer {
    fn prices(&self, preset: &Preset) -> Vec<(Coefficient, f64)> {
        Coefficient::variants()
            .into_iter()
            .filter(|c| c != &Coefficient::Initial)
            .filter_map(|c| match preset.usage_coeffs.get(&c) {
                Some(v) => Some((c, *v)),
                None => None,
            })
            .collect()
    }

    fn build(
        &self,
        accounts: &Vec<Account>,
        initial_price: f64,
        prices: Vec<(String, f64)>,
    ) -> Result<ComInfo> {
        let mut usage_vector = Vec::new();
        let coefficients = prices
            .into_iter()
            .map(|(p, v)| {
                usage_vector.push(p);
                v
            })
            .chain(std::iter::once(initial_price))
            .collect::<Vec<_>>();

        let mut params = json!({
            "scheme": "payu".to_string(),
            "scheme.payu": json!({
                "interval_sec": self.interval
            }),
            "pricing": json!({
                "model": "linear".to_string(),
                "model.linear": json!({
                    "coeffs": coefficients
                })
            }),
            "usage": json!({
                "vector": usage_vector
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

        Ok(ComInfo { params })
    }
}
