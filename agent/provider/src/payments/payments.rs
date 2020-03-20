use actix::prelude::*;
use anyhow::{anyhow, Error, Result};
use bigdecimal::BigDecimal;
use chrono::Utc;
use log;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::model::{PaymentDescription, PaymentModel};
use crate::execution::{ActivityCreated, ActivityDestroyed};
use crate::market::provider_market::AgreementApproved;
use crate::payments::factory::PaymentModelFactory;

use ya_client::activity::ActivityProviderApi;
use ya_client::payment::provider::ProviderApi;
use ya_model::market::Agreement;
use ya_model::payment::{DebitNote, Invoice, NewDebitNote, NewInvoice};
use ya_utils_actix::actix_handler::{send_message, ResultTypeGetter};
use ya_utils_actix::forward_actix_handler;

// =========================================== //
// Internal messages
// =========================================== //

/// Checks activity usage counters and updates service
/// cost. Sends debit note to requestor.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
pub struct UpdateCost {
    pub invoice_info: InvoiceInfo,
}

/// Changes activity state to Finalized and stores final cost.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
pub struct FinalizeActivity {
    pub debit_info: InvoiceInfo,
    pub cost_info: CostInfo,
}

/// TODO: We should get this message from external world.
/// Computes costs for all activities and send invoice to requestor.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
pub struct AgreementClosed {
    pub agreement_id: String,
}

// =========================================== //
// Payments implementation
// =========================================== //

#[derive(Clone)]
pub struct InvoiceInfo {
    pub agreement_id: String,
    pub activity_id: String,
    pub last_debit_note: Option<String>,
}

#[derive(Clone, PartialEq)]
pub struct CostInfo {
    pub usage: Vec<f64>,
    pub cost: BigDecimal,
}

#[derive(PartialEq)]
enum ActivityPayment {
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
        last_debit_note: Option<String>,
        /// Option in case we didn't sent any debit notes.
        cost_info: CostInfo,
    },
}

/// Payment information related to single agreement.
/// Note that we can have multiple activities during duration of agreement.
/// We must wait until agreement will be closed, before we send invoice.
struct AgreementPayment {
    agreement_id: String,
    update_interval: Duration,
    payment_model: Arc<Box<dyn PaymentModel>>,
    activities: HashMap<String, ActivityPayment>,
}

/// Yagna APIs and payments information about provider.
struct ProviderCtx {
    activity_api: Arc<ActivityProviderApi>,
    payment_api: Arc<ProviderApi>,

    credit_account: String,
}

/// Computes charges for tasks execution.
/// Sends payments events to requestor through payment API.
pub struct Payments {
    context: Arc<ProviderCtx>,
    agreements: HashMap<String, AgreementPayment>,
}

impl Payments {
    pub fn new(
        activity_api: ActivityProviderApi,
        payment_api: ProviderApi,
        credit_address: &str,
    ) -> Payments {
        log::info!(
            "Payments will be sent to account address {}.",
            credit_address
        );

        let provider_ctx = ProviderCtx {
            activity_api: Arc::new(activity_api),
            payment_api: Arc::new(payment_api),
            credit_account: credit_address.to_string(),
        };

        Payments {
            agreements: HashMap::new(),
            context: Arc::new(provider_ctx),
        }
    }

    pub fn on_signed_agreement(&mut self, msg: AgreementApproved, _ctx: &mut Context<Self>) -> Result<()> {
        log::info!(
            "Payments got signed agreement [{}]. Waiting for activities creation...",
            &msg.agreement.agreement_id
        );

        match AgreementPayment::new(&msg.agreement) {
            Ok(activity) => {
                self.agreements
                    .insert(msg.agreement.agreement_id.clone(), activity);
                Ok(())
            }
            Err(error) => {
                log::error!(
                    "Failed to create payment model for agreement [{}]. Error: {}",
                    &msg.agreement.agreement_id,
                    error
                );
                Err(error)
            }
        }
    }

    fn update_debit_note(
        &mut self,
        agreement_id: &str,
        activity_id: &str,
        debit_note_id: Option<String>,
    ) -> Result<()> {
        let activity = self
            .agreements
            .get_mut(agreement_id)
            .ok_or(anyhow!("Can't find agreement [{}].", agreement_id))?
            .activities
            .get_mut(activity_id)
            .ok_or(anyhow!(
                "Can't find activity [{}] for agreement [{}].",
                activity_id,
                agreement_id
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
}

async fn compute_cost(
    payment_model: Arc<Box<dyn PaymentModel>>,
    provider_context: Arc<ProviderCtx>,
    activity_id: String,
) -> Result<CostInfo> {
    // let activity_api = provider_context.activity_api.clone();
    // let usage = activity_api
    //     .get_activity_usage(&activity_id)
    //     .await
    //     .map_err(|error| {
    //         anyhow!(
    //             "Can't query usage for activity [{}]. Error: {}",
    //             &activity_id,
    //             error
    //         )
    //     })?
    //     .current_usage
    //     .ok_or(anyhow!(
    //         "Empty usage vector for activity [{}].",
    //         &activity_id
    //     ))?;

    let usage = vec![1.0, 1.0];
    let cost = payment_model.compute_cost(&usage)?;

    Ok(CostInfo { cost, usage })
}

async fn send_debit_note(
    payment_model: Arc<Box<dyn PaymentModel>>,
    provider_context: Arc<ProviderCtx>,
    invoice_info: InvoiceInfo,
    cost_info: CostInfo,
) -> Result<DebitNote> {
    let debit_note = NewDebitNote {
        agreement_id: invoice_info.agreement_id.clone(),
        activity_id: Some(invoice_info.activity_id.clone()),
        previous_debit_note_id: invoice_info.last_debit_note.clone(),
        total_amount_due: cost_info.cost,
        usage_counter_vector: Some(json!(cost_info.usage)),
        credit_account_id: provider_context.credit_account.clone(),
        payment_platform: None,
        payment_due_date: None,
    };

    log::debug!(
        "Creating debit note {}.",
        serde_json::to_string(&debit_note)?
    );

    let payment_api = provider_context.payment_api.clone();
    let debit_note = payment_api
        .issue_debit_note(&debit_note)
        .await
        .map_err(|error| {
            anyhow!(
                "Failed to issue debit note for activity [{}]. {}",
                &invoice_info.activity_id,
                error
            )
        })?;

    log::debug!(
        "Sending debit note [{}] for activity [{}].",
        &debit_note.debit_note_id,
        &invoice_info.activity_id
    );
    payment_api
        .send_debit_note(&debit_note.debit_note_id)
        .await
        .map_err(|error| {
            anyhow!(
                "Failed to send debit note [{}] for activity [{}]. {}",
                &debit_note.debit_note_id,
                &invoice_info.activity_id,
                error
            )
        })?;

    log::info!(
        "Debit note [{}] for activity [{}] sent.",
        &debit_note.debit_note_id,
        &invoice_info.activity_id
    );

    Ok(debit_note)
}

async fn send_invoice(
    payment_model: Arc<Box<dyn PaymentModel>>,
    provider_context: Arc<ProviderCtx>,
    invoice_info: InvoiceInfo,
    cost_info: CostInfo,
    activities: Vec<String>,
) -> Result<Invoice> {
    log::info!(
        "Final cost for agreement [{}]: {}, usage {:?}.",
        &invoice_info.agreement_id,
        &cost_info.cost,
        &cost_info.usage
    );

    let invoice = NewInvoice {
        agreement_id: invoice_info.agreement_id.clone(),
        activity_ids: Some(activities),
        amount: cost_info.cost,
        usage_counter_vector: Some(json!(cost_info.usage)),
        credit_account_id: provider_context.credit_account.clone(),
        payment_platform: None,
        payment_due_date: Utc::now(),
    };

    log::debug!("Creating invoice {}.", serde_json::to_string(&invoice)?);

    let payment_api = provider_context.payment_api.clone();
    let invoice = payment_api.issue_invoice(&invoice).await.map_err(|error| {
        anyhow!(
            "Failed to issue debit note for agreement [{}]. {}",
            &invoice_info.agreement_id,
            error
        )
    })?;

    log::debug!(
        "Sending invoice [{}] for agreement [{}].",
        &invoice.invoice_id,
        &invoice_info.agreement_id
    );
    payment_api
        .send_invoice(&invoice.invoice_id)
        .await
        .map_err(|error| {
            anyhow!(
                "Failed to send invoice [{}] for agreement [{}]. {}",
                &invoice.invoice_id,
                &invoice_info.agreement_id,
                error
            )
        })?;

    log::info!(
        "Invoice [{}] for agreement [{}] sent.",
        &invoice.invoice_id,
        &invoice_info.agreement_id
    );

    Ok(invoice)
}

forward_actix_handler!(Payments, AgreementApproved, on_signed_agreement);

impl Handler<ActivityCreated> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityCreated, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.agreement_id) {
            log::info!(
                "Payments - activity [{}] created. Start computing costs.",
                &msg.activity_id
            );

            // Sending UpdateCost with last_debit_note: None will start new
            // DebitNotes chain for this activity.
            let msg = UpdateCost {
                invoice_info: InvoiceInfo {
                    agreement_id: msg.agreement_id.clone(),
                    activity_id: msg.activity_id.clone(),
                    last_debit_note: None,
                },
            };

            // Add activity to list and send debit note after update_interval.
            agreement.activity_created(&msg.invoice_info.activity_id);
            ctx.notify_later(msg, agreement.update_interval);

            ActorResponse::reply(Ok(()))
        } else {
            ActorResponse::reply(Err(anyhow!(
                "Agreement [{}] wasn't registered.",
                &msg.agreement_id
            )))
        }
    }
}

impl Handler<ActivityDestroyed> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityDestroyed, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.agreement_id) {
            agreement.activity_destroyed(&msg.activity_id).unwrap();

            if let Some(ActivityPayment::Destroyed {
                last_debit_note, ..
            }) = agreement.activities.get(&msg.activity_id)
            {
                let payment_model = agreement.payment_model.clone();
                let provider_context = self.context.clone();
                let address = ctx.address();
                let invoice_info = InvoiceInfo {
                    activity_id: msg.activity_id.clone(),
                    agreement_id: msg.agreement_id.clone(),
                    last_debit_note: last_debit_note.clone(),
                };

                let future = async move {
                    let cost_info = compute_cost(
                        payment_model.clone(),
                        provider_context.clone(),
                        msg.activity_id.clone(),
                    )
                    .await?;

                    log::info!(
                        "Final cost for activity [{}]: {}.",
                        &msg.activity_id,
                        &cost_info.cost
                    );

                    send_debit_note(
                        payment_model,
                        provider_context,
                        invoice_info.clone(),
                        cost_info.clone(),
                    )
                    .await?;

                    let msg = FinalizeActivity {
                        cost_info,
                        debit_info: invoice_info,
                    };

                    address.send(msg).await??;
                    Ok(())
                }
                .into_actor(self);

                return ActorResponse::r#async(future);
            } else {
                log::error!("Shouldn't happen.");
            }
        }

        log::warn!(
            "Can't find activity [{}] and agreement [{}].",
            &msg.activity_id,
            &msg.agreement_id
        );
        return ActorResponse::reply(Err(anyhow!("")));
    }
}

impl Handler<UpdateCost> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: UpdateCost, _ctx: &mut Context<Self>) -> Self::Result {
        let agreement = match self.agreements.get(&msg.invoice_info.agreement_id) {
            Some(agreement) => agreement,
            None => {
                log::warn!(
                    "Not my activity - agreement [{}].",
                    &msg.invoice_info.agreement_id
                );
                return ActorResponse::reply(Err(anyhow!("")));
            }
        };

        if let Some(activity) = agreement.activities.get(&msg.invoice_info.activity_id) {
            if let ActivityPayment::Running { .. } = activity {
                let payment_model = agreement.payment_model.clone();
                let context = self.context.clone();
                let msg = msg.clone();
                let update_interval = agreement.update_interval;

                return ActorResponse::r#async(
                    async move {
                        let cost_info = compute_cost(
                            payment_model.clone(),
                            context.clone(),
                            msg.invoice_info.activity_id.clone(),
                        )
                        .await?;

                        log::info!(
                            "Updating cost for activity [{}]: {}.",
                            &msg.invoice_info.activity_id,
                            &cost_info.cost
                        );

                        send_debit_note(
                            payment_model.clone(),
                            context.clone(),
                            msg.invoice_info,
                            cost_info,
                        )
                        .await
                    }
                    .into_actor(self)
                    .map(move |result, sself, ctx| {
                        match result {
                            Ok(debit_note) => {
                                // msg contains updated debit_note_id.
                                let activity_id = debit_note.activity_id.unwrap();
                                sself.update_debit_note(
                                    &debit_note.agreement_id,
                                    &activity_id,
                                    debit_note.previous_debit_note_id.clone(),
                                )?;

                                let msg = UpdateCost {
                                    invoice_info: InvoiceInfo {
                                        agreement_id: debit_note.agreement_id.clone(),
                                        activity_id,
                                        last_debit_note: debit_note.previous_debit_note_id.clone(),
                                    },
                                };
                                ctx.notify_later(msg, update_interval);
                                Ok(())
                            }
                            Err(error) => {
                                log::error!("{}", error);
                                return Err(error);
                            }
                        }
                    }),
                );
            } else {
                // Note: we don't send here new UpdateCost message, what stops further updates.
                log::info!(
                    "Stopped sending debit notes, because activity [{}] was destroyed.",
                    &msg.invoice_info.activity_id
                );
                return ActorResponse::reply(Ok(()));
            }
        }
        return ActorResponse::reply(Err(anyhow!(
            "We shouldn't be here. Activity [{}], not found.",
            &msg.invoice_info.activity_id
        )));
    }
}

impl Handler<FinalizeActivity> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: FinalizeActivity, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.debit_info.agreement_id) {
            log::info!("Activity [{}] finished.", &msg.debit_info.activity_id);

            let result = agreement.finish_activity(&msg.debit_info.activity_id, msg.cost_info);

            // Temporary. Requestor should close agreement, but for now we
            // treat destroying activity as closing agreement.
            send_message(
                ctx.address(),
                AgreementClosed {
                    agreement_id: msg.debit_info.agreement_id.clone(),
                },
            );

            return ActorResponse::reply(result);
        } else {
            log::warn!(
                "Not my activity - agreement [{}].",
                &msg.debit_info.agreement_id
            );
            return ActorResponse::reply(Err(anyhow!("")));
        }
    }
}

impl Handler<AgreementClosed> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementClosed, _ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.agreement_id) {
            log::info!(
                "Payments - agreement [{}] closed. Computing cost summary...",
                &msg.agreement_id
            );

            let cost_summary = agreement.cost_summary();
            let activities = agreement.list_activities();
            let payment_model = agreement.payment_model.clone();
            let provider_context = self.context.clone();

            let invoice_info = InvoiceInfo {
                activity_id: "".to_string(),
                agreement_id: msg.agreement_id.clone(),
                last_debit_note: None,
            };

            let future = async move {
                let result = send_invoice(
                    payment_model,
                    provider_context,
                    invoice_info,
                    cost_summary,
                    activities,
                )
                .await;
                match result {
                    Err(error) => log::error!("{}", error),
                    _ => (),
                }
                Ok(())
            }
            .into_actor(self);

            return ActorResponse::r#async(future);
        }

        return ActorResponse::reply(Err(anyhow!("Not my agreement {}.", &msg.agreement_id)));
    }
}

impl Actor for Payments {
    type Context = Context<Self>;
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

    pub fn activity_created(&mut self, activity_id: &str) {
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
                    cost_info,
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
                    ActivityPayment::Finalized { cost_info, .. } => {
                        Some((&cost_info.cost, &cost_info.usage))
                    }
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

    pub fn list_activities(&self) -> Vec<String> {
        self.activities
            .iter()
            .map(|(activity_id, _)| activity_id.clone())
            .collect()
    }
}
