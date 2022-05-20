use anyhow::{anyhow, bail, Result};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::watch;

use super::factory::PaymentModelFactory;
use super::model::{PaymentDescription, PaymentModel};
use crate::display::EnableDisplay;

use ya_agreement_utils::AgreementView;
use ya_client::activity::ActivityProviderApi;

const PAYMENT_PRECISION: i64 = 18; // decimal places

#[derive(Clone, PartialEq)]
pub struct CostInfo {
    pub usage: Vec<f64>,
    pub cost: BigDecimal,
}

impl CostInfo {
    pub fn new(usage: Vec<f64>, cost: BigDecimal) -> Self {
        let cost = cost.round(PAYMENT_PRECISION);
        CostInfo { usage, cost }
    }
}

#[derive(PartialEq)]
//TODO: Remove last_debit_note in future. Payment api
//      should deduce it based on activity id.
pub enum ActivityPayment {
    /// We got activity created event.
    Running { activity_id: String },
    /// We got activity destroyed event, but cost still isn't computed.
    Destroyed { activity_id: String },
    /// We computed cost and sent last debit note. Activity should
    /// never change from this moment.
    Finalized {
        activity_id: String,
        cost_summary: CostInfo,
    },
}

/// Payment information related to single agreement.
/// Note that we can have multiple activities during duration of agreement.
/// We must wait until agreement will be closed, before we send invoice.
pub struct AgreementPayment {
    pub agreement_id: String,
    pub approved_ts: DateTime<Utc>,
    pub payment_model: Arc<dyn PaymentModel>,
    pub activities: HashMap<String, ActivityPayment>,

    pub update_interval: std::time::Duration,
    pub accept_timeout: Option<chrono::Duration>,
    pub payment_timeout: Option<chrono::Duration>,
    // If at least one deadline elapses, we don't want to generate any
    // new unnecessary events.
    pub deadline_elapsed: bool,
    // If we are unable to send DebitNotes, we should break Agreement the same
    // way as in case, when Requestor doesn't accept them.
    pub last_send_debit_note: DateTime<Utc>,
    // Keep track of the last payable debit note so we can send a payable
    // debit note each payment_timeout
    pub last_payable_debit_note: DateTime<Utc>,

    // Watches for waiting for activities. You can await on receiver
    // to observe changes in number of active activities.
    pub watch_sender: watch::Sender<usize>,
    pub activities_watch: ActivitiesWaiter,
}

#[derive(Clone)]
pub struct ActivitiesWaiter {
    watch_receiver: watch::Receiver<usize>,
}

impl AgreementPayment {
    pub fn new(agreement: &AgreementView) -> Result<AgreementPayment> {
        let payment_description = PaymentDescription::new(agreement)?;
        let payment_model = PaymentModelFactory::create(&payment_description)?;
        let update_interval = payment_description.get_update_interval()?;
        let accept_timeout = payment_description.get_debit_note_accept_timeout()?;
        let payment_timeout = payment_description.get_payment_timeout()?;
        let approved_ts = payment_description.get_approved_ts()?;

        if let Some(deadline) = &accept_timeout {
            log::info!(
                "Requestor is expected to accept DebitNotes for Agreement [{}] in {}",
                &agreement.agreement_id,
                deadline.display()
            );
        }
        if let Some(deadline) = &payment_timeout {
            log::info!(
                "Requestor is expected to pay DebitNotes for Agreement [{}] in {}",
                &agreement.agreement_id,
                deadline.display()
            );
        }

        // Initially we have 0 activities.
        let (sender, receiver) = watch::channel(0);

        Ok(AgreementPayment {
            agreement_id: agreement.agreement_id.clone(),
            approved_ts,
            activities: HashMap::new(),
            payment_model,
            update_interval,
            accept_timeout,
            payment_timeout,
            deadline_elapsed: false,
            last_send_debit_note: approved_ts,
            last_payable_debit_note: approved_ts,
            watch_sender: sender,
            activities_watch: ActivitiesWaiter {
                watch_receiver: receiver,
            },
        })
    }

    pub fn add_created_activity(&mut self, activity_id: &str) {
        let activity = ActivityPayment::Running {
            activity_id: activity_id.to_string(),
        };
        self.activities.insert(activity_id.to_string(), activity);

        // Send number of activities. ActivitiesWaiter can be than awaited
        // until required condition is met.
        let num_activities = self.count_active_activities();
        let _ = self.watch_sender.send(num_activities);
    }

    pub fn activity_destroyed(&mut self, activity_id: &str) -> Result<()> {
        if let Some(activity) = self.activities.get_mut(activity_id) {
            if let ActivityPayment::Running { activity_id } = activity {
                *activity = ActivityPayment::Destroyed {
                    activity_id: activity_id.clone(),
                };
                return Ok(());
            }
        }
        Err(anyhow!("Activity [{}] didn't exist before.", activity_id))
    }

    pub fn finish_activity(&mut self, activity_id: &str, cost_info: CostInfo) -> Result<()> {
        if cost_info.usage.len() != self.payment_model.expected_usage_len() {
            return Err(anyhow!(
                "Usage vector has length {}, but expected {}.",
                cost_info.usage.len(),
                self.payment_model.expected_usage_len()
            ));
        }

        if let Some(activity) = self.activities.get_mut(activity_id) {
            if let ActivityPayment::Destroyed { activity_id } = activity {
                *activity = ActivityPayment::Finalized {
                    activity_id: activity_id.clone(),
                    cost_summary: cost_info,
                };

                // Send number of activities. ActivitiesWaiter can be than awaited
                // until required condition is met.
                let num_activities = self.count_active_activities();
                self.watch_sender.send(num_activities)?;

                return Ok(());
            }
        }
        Err(anyhow!("Activity [{}] didn't exist before.", activity_id))
    }

    pub fn count_active_activities(&self) -> usize {
        self.activities
            .iter()
            .filter(|(_, activity)| match activity {
                ActivityPayment::Finalized { .. } => false,
                _ => true,
            })
            .count()
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

        CostInfo::new(usage, cost)
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
        .current_usage;

    // Empty usage vector can occur, when ExeUnit didn't send
    // any metric yet. We can handle this as usage with all values
    // set to 0.0. Note that cost in this case can be not zero, as
    // there's constant coefficient.
    let usage = match usage {
        Some(usage_vec) => usage_vec,
        None => vec![0.0; payment_model.expected_usage_len()],
    };

    if usage.len() != payment_model.expected_usage_len() {
        bail!(
            "Incorrect usage vector length {} for activity [{}]. Expected: {}.",
            usage.len(),
            activity_id,
            payment_model.expected_usage_len()
        );
    }

    let cost = payment_model.compute_cost(&usage)?;

    Ok(CostInfo::new(usage, cost))
}

impl ActivitiesWaiter {
    pub async fn wait_for_finish(mut self) {
        log::debug!("Waiting for all activities to finish.");
        while let Some(value) = self
            .watch_receiver
            .changed()
            .await
            .map(|_| *self.watch_receiver.borrow())
            .ok()
        {
            log::debug!("Num active activities left: {}.", value);
            if value == 0 {
                log::debug!("All activities finished.");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_round() {
        let x = BigDecimal::from_str("12345.123456789").unwrap();
        let y = BigDecimal::from_str("12345.12346").unwrap();
        assert_eq!(x.round(5), y);

        let x = BigDecimal::from_str("12345.123456789").unwrap();
        let y = BigDecimal::from_str("12345").unwrap();
        assert_eq!(x.round(0), y);

        let x = BigDecimal::from_str("12345").unwrap();
        let y = BigDecimal::from_str("12345").unwrap();
        assert_eq!(x.round(15), y);
    }
}
