use anyhow::{anyhow, Result};
use bigdecimal::{BigDecimal, FromPrimitive};
use serde_json::json;

use ya_agreement_utils::ComInfo;
use ya_client::model::{payment::Account, NodeId};
use ya_core_model::payment::local::NetworkName;

use super::model::{PaymentDescription, PaymentModel};
use crate::market::presets::Preset;

#[derive(Clone, Debug)]
pub struct AccountView {
    pub address: NodeId,
    pub network: NetworkName,
    pub platform: String,
}

impl From<Account> for AccountView {
    fn from(account: Account) -> Self {
        Self {
            address: account.address.parse().unwrap(), // TODO: use TryFrom
            network: account.network.parse().unwrap(), // TODO: use TryFrom
            platform: account.platform,
        }
    }
}

pub trait PricingOffer {
    fn prices(&self, preset: &Preset) -> Vec<(String, f64)>;
    fn build(
        &self,
        accounts: &[AccountView],
        initial_price: f64,
        prices: Vec<(String, f64)>,
    ) -> Result<ComInfo>;
}

/// Computes computations costs.
pub struct LinearPricing {
    usage_coeffs: Vec<f64>,
}

impl PaymentModel for LinearPricing {
    fn compute_cost(&self, usage: &[f64]) -> Result<BigDecimal> {
        // Note: last element of usage_coeffs contains constant initial cost
        // of computing task, so we don't multiply it.
        let const_coeff_idx = self.usage_coeffs.len() - 1;
        let cost: f64 = self.usage_coeffs[const_coeff_idx]
            + self.usage_coeffs[0..const_coeff_idx]
                .iter()
                .zip(usage.iter())
                .map(|(coeff, usage_value)| coeff * usage_value)
                .sum::<f64>();

        BigDecimal::from_f64(cost)
            .ok_or_else(|| anyhow!("Failed to convert to BigDecimal: {}", cost))
    }

    fn expected_usage_len(&self) -> usize {
        self.usage_coeffs.len() - 1
    }
}

impl LinearPricing {
    pub fn new<'a>(commercials: &'a PaymentDescription<'a>) -> Result<LinearPricing> {
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
        LinearPricingOffer { interval: 120.0 }
    }
}

impl LinearPricingOffer {
    #[allow(unused)]
    pub fn interval(mut self, seconds: f64) -> Self {
        self.interval = seconds;
        self
    }
}

impl PricingOffer for LinearPricingOffer {
    fn prices(&self, preset: &Preset) -> Vec<(String, f64)> {
        preset.usage_coeffs.clone().into_iter().collect()
    }

    fn build(
        &self,
        accounts: &[AccountView],
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
            "scheme.payu": json!({}),
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
