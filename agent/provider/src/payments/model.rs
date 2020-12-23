use anyhow::{anyhow, Result};
use bigdecimal::BigDecimal;
use serde_json::json;
use std::time::Duration;

use ya_agreement_utils::AgreementView;

/// Implementation of payment model which knows, how to compute amount
/// of money, that requestor should pay for computations.
pub trait PaymentModel {
    fn compute_cost(&self, usage: &Vec<f64>) -> Result<BigDecimal>;
    fn expected_usage_len(&self) -> usize;
}

/// Extracted commercial part of agreement.
pub struct PaymentDescription {
    pub commercial_agreement: AgreementView,
}

impl PaymentDescription {
    pub fn new(agreement: &AgreementView) -> Result<PaymentDescription> {
        log::debug!("Agreement: {:?}", agreement.json);

        // Get rid of non commercial part of agreement.
        let commercial = agreement
            .pointer("/offer/properties/golem/com")
            .ok_or(anyhow!("No commercial properties."))?
            .clone();

        // Rebuild structure. Properties will be still visible under full paths.
        let commercial = json!({
            "offer": {
                "properties": {
                    "golem": {
                        "com": commercial
                    }
                }
            }
        });

        log::debug!("Commercial properties:\n{:#?}", &commercial);

        Ok(PaymentDescription {
            commercial_agreement: AgreementView {
                json: commercial,
                id: agreement.id.clone(),
            },
        })
    }

    pub fn get_update_interval(&self) -> Result<Duration> {
        let interval_addr = "/offer/properties/golem/com/scheme/payu/interval_sec";
        let interval = self
            .commercial_agreement
            .pointer_typed::<f64>(interval_addr)?;
        Ok(Duration::from_secs_f64(interval))
    }

    pub fn get_usage_coefficients(&self) -> Result<Vec<f64>> {
        let coeffs_addr = "/offer/properties/golem/com/pricing/model/linear/coeffs";
        Ok(self
            .commercial_agreement
            .pointer_typed::<Vec<f64>>(coeffs_addr)?)
    }
}
