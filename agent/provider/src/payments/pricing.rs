use anyhow::{anyhow, Result};
use bigdecimal::BigDecimal;
use serde_json::json;
use std::convert::TryFrom;

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
    usage_coeffs: Vec<BigDecimal>,
}

impl PaymentModel for LinearPricing {
    fn compute_cost(&self, usage: &[f64]) -> Result<BigDecimal> {
        let usage: Vec<BigDecimal> = usage
            .iter()
            .cloned()
            .map(BigDecimal::try_from)
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| anyhow!("Failed to convert usage to BigDecimal: {e}"))?;

        // Note: last element of usage_coeffs contains constant initial cost
        // of computing task, so we don't multiply it.
        let const_coeff_idx = self.usage_coeffs.len() - 1;
        Ok(self.usage_coeffs[const_coeff_idx].clone()
            + self.usage_coeffs[0..const_coeff_idx]
                .iter()
                .zip(usage.iter())
                .map(|(coeff, usage_value)| coeff * usage_value)
                .sum::<BigDecimal>())
    }

    fn expected_usage_len(&self) -> usize {
        self.usage_coeffs.len() - 1
    }
}

impl LinearPricing {
    pub fn new<'a>(commercials: &'a PaymentDescription<'a>) -> Result<LinearPricing> {
        let usage: Vec<BigDecimal> = commercials
            .get_usage_coefficients()?
            .into_iter()
            .map(BigDecimal::try_from)
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| anyhow!("Failed to convert usage coefficients to BigDecimal: {e}"))?;

        log::info!("Creating LinearPricing payment model. Usage coefficients vector: {usage:?}.");

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

#[cfg(test)]
mod tests {
    use bigdecimal::BigDecimal;
    use std::convert::TryFrom;
    use std::str::FromStr;
    use test_case::test_case;

    use crate::payments::model::{PaymentDescription, PaymentModel};
    use crate::payments::LinearPricing;

    use ya_agreement_utils::agreement::try_from_json;
    use ya_agreement_utils::AgreementView;
    use ya_framework_basic::template::template;

    const AGREEMENT_TEMPLATE: &str = r#"
{
  "agreementId": "0ec929f5acc8f98a47ab72d61a2c2f343d45d8438d3aa4ccdc84e717c219e185",
  "proposedSignature": "NoSignature",
  "state": "Pending",
  "timestamp": "2022-05-22T10:41:42.564784259Z",
  "validTo": "2022-05-22T11:41:42.562457Z",

  "offer": {
    "properties": {
      "golem.com.payment.debit-notes.accept-timeout?": 10,
      "golem.com.payment.platform.erc20-goerli-tglm.address": "0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a",
      "golem.com.pricing.model": "linear",
      "golem.com.pricing.model.linear.coeffs": [${coeffs}],
      "golem.com.scheme": "payu",
      "golem.com.scheme.payu.debit-note.interval-sec?": 1,
      "golem.com.scheme.payu.payment-timeout-sec?": 10,
      "golem.com.usage.vector": [
        "golem.usage.cpu_sec",
        "golem.usage.duration_sec"
      ]
    },
    "constraints": "(&\n  (golem.srv.comp.expiration>1705586871777)\n)",
    "offerId": "afce49b1ea5b45db91bdd6e5481479f9-9095fca9dea0a91ce95cf994125b33cdd838fcc963a1106f2be9e4b5b65a52f0",
    "providerId": "0x86a269498fb5270f20bdc6fdcf6039122b0d3b23",
    "timestamp": "2022-05-22T10:41:42.564784259Z"
  },

  "demand": {
    "constraints": "(&(golem.com.payment.platform.erc20-goerli-tglm.address=*)\n\t(golem.com.pricing.model=linear)\n\t(&(golem.inf.mem.gib>=0.5)\n\t(golem.inf.storage.gib>=2.0)\n\t(golem.inf.cpu.threads>=1)\n\t(golem.runtime.name=ya-mock-runtime)))",
    "demandId": "773035fc685c46da8e61473ac2a2568e-3f3eb86d6ef9a01708d0f57d0b19cc69fd74422150c120e33cc1b5f4a1a12b96",
    "properties": {},
    "requestorId": "0xa5ad3f81e283983b8e9705b2e31d0c138bb2b1b7",
    "timestamp": "2022-05-22T10:41:42.564784259Z"
  }
}
"#;

    #[test_case(
        "0.0001, 0.00005, 0.0",
        &[44.017951, 103.002864998],
        BigDecimal::from_str("0.0095519383499").unwrap();
        "Check non-represented float values"
    )]
    #[test_case(
        "0.002, 0.008, 0.0",
        &[44.094619588, 0.0],
        BigDecimal::from_str("0.088189239176").unwrap();
        "Check overflowing example"
    )]
    #[test_case(
        "0.002, 0.008, 0.0",
        &[24.141030488, 0.0],
        BigDecimal::from_str("0.048282060976").unwrap();
        "Check underflowing example"
    )]
    fn test_linear_payment_model_cost(coeffs: &str, usage: &[f64], expected: BigDecimal) {
        let agreement = AgreementView::try_from(
            try_from_json(template(
                AGREEMENT_TEMPLATE,
                &[("coeffs", coeffs.to_string())],
            ))
            .unwrap(),
        )
        .unwrap();
        let payment = PaymentDescription::new(&agreement).unwrap();
        let pricing = LinearPricing::new(&payment).unwrap();

        assert_eq!(pricing.compute_cost(usage).unwrap(), expected);
    }
}
