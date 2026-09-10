use anyhow::{anyhow, Result};
use bigdecimal::BigDecimal;
use serde_json::json;
use std::str::FromStr;

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
    fn prices(&self, preset: &Preset) -> Vec<(String, BigDecimal)>;
    fn build(
        &self,
        accounts: &[AccountView],
        initial_price: BigDecimal,
        prices: Vec<(String, BigDecimal)>,
    ) -> Result<ComInfo>;
}

/// Computes computations costs.
pub struct LinearPricing {
    usage_coeffs: Vec<BigDecimal>,
}

fn usage_decimal_from_f64(
    value: f64,
) -> std::result::Result<BigDecimal, bigdecimal::ParseBigDecimalError> {
    // Preserve the conversion precision used by BigDecimal 0.2. Its 0.4
    // conversion keeps the exact binary float value instead.
    BigDecimal::from_str(&format!(
        "{value:.precision$e}",
        precision = f64::DIGITS as usize
    ))
}

impl PaymentModel for LinearPricing {
    fn compute_cost(&self, usage: &[f64]) -> Result<BigDecimal> {
        let usage: Vec<BigDecimal> = usage
            .iter()
            .cloned()
            .map(usage_decimal_from_f64)
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
        let usage_coeffs = commercials.get_usage_coefficients()?;

        log::info!(
            "Creating LinearPricing payment model. Usage coefficients vector: {usage_coeffs:?}."
        );

        Ok(LinearPricing { usage_coeffs })
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
    fn prices(&self, preset: &Preset) -> Vec<(String, BigDecimal)> {
        preset.usage_coeffs.clone().into_iter().collect()
    }

    fn build(
        &self,
        accounts: &[AccountView],
        initial_price: BigDecimal,
        prices: Vec<(String, BigDecimal)>,
    ) -> Result<ComInfo> {
        let mut usage_vector = Vec::new();
        let coefficients = prices
            .into_iter()
            .map(|(p, v)| {
                usage_vector.push(p);
                v
            })
            .chain(std::iter::once(initial_price))
            .map(|value| {
                // Market pricing coefficients are part of the public offer
                // protocol and requestors (including yapapi) require floats.
                // Keep BigDecimal in presets and payment calculations, but
                // deliberately convert at this protocol boundary.
                let value = value
                    .to_plain_string()
                    .parse::<f64>()
                    .map_err(|e| anyhow!("Failed to convert price coefficient to float: {e}"))?;
                serde_json::Number::from_f64(value)
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| anyhow!("Price coefficient is not a finite float"))
            })
            .collect::<Result<Vec<_>>>()?;

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
    use crate::payments::{LinearPricing, LinearPricingOffer, PricingOffer};

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
      "golem.com.payment.debit-notes.accept-timeout?": 240,
      "golem.com.payment.platform.erc20-goerli-tglm.address": "0x95369fc6fd02afeca110b9c32a21fb8ad899ee0a",
      "golem.com.pricing.model": "linear",
      "golem.com.pricing.model.linear.coeffs": [${coeffs}],
      "golem.com.scheme": "payu",
      "golem.com.scheme.payu.debit-note.interval-sec?": 120,
      "golem.com.scheme.payu.payment-timeout-sec?": 120,
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

    #[test]
    fn pricing_offer_serializes_wire_compatible_numeric_coefficients() {
        let exact = BigDecimal::from_str("0.12345678901234567890123456789").unwrap();
        let offer = LinearPricingOffer::default()
            .build(&[], BigDecimal::from(0), vec![("counter".into(), exact)])
            .unwrap();
        let properties = ya_agreement_utils::agreement::flatten_value(serde_json::json!({
            "golem": { "com": offer.params }
        }));
        let coefficients = properties
            .get("golem.com.pricing.model.linear.coeffs")
            .unwrap();

        assert_eq!(coefficients.to_string(), "[0.12345678901234568,0.0]");
        assert!(coefficients
            .as_array()
            .unwrap()
            .iter()
            .all(serde_json::Value::is_f64));
    }

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
    #[test_case(
        "0, 0, 0.12345678901234567890123456789",
        &[0.0, 0.0],
        BigDecimal::from_str("0.12345678901234568").unwrap();
        "Use the exact decimal representation carried by agreement JSON"
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
