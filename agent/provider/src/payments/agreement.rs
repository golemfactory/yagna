use anyhow::{anyhow, Result};
use bigdecimal::BigDecimal;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::factory::PaymentModelFactory;
use super::model::{PaymentDescription, PaymentModel};

use ya_client::activity::ActivityProviderApi;
use ya_model::market::Agreement;

#[derive(Clone, PartialEq)]
pub struct CostInfo {
    pub usage: Vec<f64>,
    pub cost: BigDecimal,
}

#[derive(PartialEq)]
//TODO: Remove last_debit_note in future. Payment api
//      should deduce it based on activity id.
pub enum ActivityPayment {
    /// We got activity created event.
    Running {
        activity_id: String,
        last_debit_note: Option<String>,
    },
    /// We got activity destroyed event, but cost still isn't computed.
    Destroyed {
        activity_id: String,
        last_debit_note: Option<String>,
    },
    /// We computed cost and sent last debit note. Activity should
    /// never change from this moment.
    Finalized {
        activity_id: String,
        /// Option in case, we didn't sent any debit notes.
        last_debit_note: Option<String>,
        cost_summary: CostInfo,
    },
}

/// Payment information related to single agreement.
/// Note that we can have multiple activities during duration of agreement.
/// We must wait until agreement will be closed, before we send invoice.
pub struct AgreementPayment {
    pub agreement_id: String,
    pub update_interval: Duration,
    pub payment_model: Arc<dyn PaymentModel>,
    pub activities: HashMap<String, ActivityPayment>,
}

impl AgreementPayment {
    pub fn new(agreement: &Agreement) -> Result<AgreementPayment> {
        let payment_description = PaymentDescription::new(agreement)?;
        let update_interval = payment_description.get_update_interval()?;
        let payment_model = PaymentModelFactory::create(payment_description)?;

        Ok(AgreementPayment {
            agreement_id: agreement.agreement_id.clone(),
            activities: HashMap::new(),
            payment_model,
            update_interval,
        })
    }

    pub fn add_created_activity(&mut self, activity_id: &str) {
        let activity = ActivityPayment::Running {
            activity_id: activity_id.to_string(),
            last_debit_note: None,
        };
        self.activities.insert(activity_id.to_string(), activity);
    }

    pub fn activity_destroyed(&mut self, activity_id: &str) -> Result<()> {
        if let Some(activity) = self.activities.get_mut(activity_id) {
            if let ActivityPayment::Running {
                activity_id,
                last_debit_note,
            } = activity
            {
                return Ok(*activity = ActivityPayment::Destroyed {
                    activity_id: activity_id.clone(),
                    last_debit_note: last_debit_note.clone(),
                });
            }
        }
        Err(anyhow!("Activity [{}] didn't exist before.", activity_id))
    }

    pub fn finish_activity(&mut self, activity_id: &str, cost_info: CostInfo) -> Result<()> {
        if cost_info.usage.len() != self.payment_model.expected_usage_len() {
            return Err(anyhow!(
                "Usage vector has length {} but expected {}.",
                cost_info.usage.len(),
                self.payment_model.expected_usage_len()
            ));
        }

        if let Some(activity) = self.activities.get_mut(activity_id) {
            if let ActivityPayment::Destroyed {
                activity_id,
                last_debit_note,
            } = activity
            {
                return Ok(*activity = ActivityPayment::Finalized {
                    activity_id: activity_id.clone(),
                    last_debit_note: last_debit_note.clone(),
                    cost_summary: cost_info,
                });
            }
        }
        Err(anyhow!("Activity [{}] didn't exist before.", activity_id))
    }

    pub fn cost_summary(&self) -> CostInfo {
        // Take into account only finalized activities.
        let filtered_activities =
            self.activities
                .iter()
                .filter_map(|(_, activity)| match activity {
                    ActivityPayment::Finalized {
                        cost_summary: cost_info,
                        ..
                    } => Some((&cost_info.cost, &cost_info.usage)),
                    _ => None,
                });

        let cost: BigDecimal = filtered_activities.clone().map(|(cost, _)| cost).sum();

        let usage_len = self.payment_model.expected_usage_len();
        let usage: Vec<f64> = filtered_activities.map(|(_, usage)| usage).fold(
            vec![0.0; usage_len],
            |accumulator, usage| {
                accumulator
                    .iter()
                    .zip(usage.iter())
                    .map(|(acc, usage)| acc + usage)
                    .collect()
            },
        );

        CostInfo { cost, usage }
    }

    pub fn update_debit_note(
        &mut self,
        activity_id: &str,
        debit_note_id: Option<String>,
    ) -> Result<()> {
        let activity = self.activities.get_mut(activity_id).ok_or(anyhow!(
            "Can't find activity [{}] for agreement [{}].",
            activity_id,
            self.agreement_id
        ))?;

        if let ActivityPayment::Running { .. } = activity {
            let new_activity = ActivityPayment::Running {
                last_debit_note: debit_note_id,
                activity_id: activity_id.to_string(),
            };
            *activity = new_activity;
            Ok(())
        } else {
            Err(anyhow!(
                "Can't update debit note id for finalized activity."
            ))
        }
    }

    pub fn list_activities(&self) -> Vec<String> {
        self.activities
            .iter()
            .map(|(activity_id, _)| activity_id.clone())
            .collect()
    }
}

pub async fn compute_cost(
    payment_model: Arc<dyn PaymentModel>,
    activity_api: Arc<ActivityProviderApi>,
    activity_id: String,
) -> Result<CostInfo> {
    let usage = activity_api
        .get_activity_usage(&activity_id)
        .await
        .map_err(|error| {
            anyhow!(
                "Can't query usage for activity [{}]. Error: {}",
                &activity_id,
                error
            )
        })?
        .current_usage
        .ok_or(anyhow!(
            "Empty usage vector for activity [{}].",
            &activity_id
        ))?;

    let cost = payment_model.compute_cost(&usage)?;

    Ok(CostInfo { cost, usage })
}
