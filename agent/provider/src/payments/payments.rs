use actix::prelude::*;
use anyhow::{Result, anyhow, Error};
use log;
use serde_json::json;
use std::sync::Arc;
use std::collections::HashMap;
use std::time::Duration;

use crate::market::provider_market::AgreementApproved;
use crate::execution::{ActivityCreated, ActivityDestroyed};
use super::model::{PaymentModel, PaymentDescription};
use crate::payments::factory::PaymentModelFactory;

use ya_client::activity::ActivityProviderApi;
use ya_client::payment::provider::ProviderApi;
use ya_utils_actix::actix_handler::{ResultTypeGetter};
use ya_utils_actix::forward_actix_handler;
use ya_model::market::Agreement;
use ya_model::payment::{NewDebitNote, NewInvoice};
use bigdecimal::BigDecimal;


const UPDATE_COST_INTERVAL_MILLIS: u64 = 10000;

// =========================================== //
// Internal messages
// =========================================== //

/// Checks activity usage counters and updates service
/// cost. Sends debit note to requestor.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
pub struct UpdateCost {
    pub agreement_id: String,
    pub activity_id: String,
    pub last_debit_note: Option<String>,
}

// =========================================== //
// Payments implementation
// =========================================== //

#[derive(PartialEq)]
enum ActivityPayment {
    Running {
        activity_id: String,
        last_debit_note: Option<String>,
    },
    Destroyed {
        activity_id: String,
        last_debit_note: String,
        final_usage: Vec<f64>,
    }
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

    creadit_account: String,
}

/// Computes charges for tasks execution.
/// Sends payments events to requestor through payment API.
pub struct Payments {
    context: Arc<ProviderCtx>,
    agreements: HashMap<String, AgreementPayment>,
}

impl Payments {
    pub fn new(activity_api: ActivityProviderApi, payment_api: ProviderApi) -> Payments {
        let provider_ctx = ProviderCtx{
            activity_api: Arc::new(activity_api),
            payment_api: Arc::new(payment_api),
            creadit_account: "0xa74476443119A942dE498590Fe1f2454d7D4aC0d".to_string()
        };

        Payments{
            agreements: HashMap::new(),
            context: Arc::new(provider_ctx),
        }
    }

    pub fn on_signed_agreement(&mut self, msg: AgreementApproved) -> Result<()> {
        log::info!(
            "Payments got signed agreement [{}]. Waiting for activities creation...",
            &msg.agreement.agreement_id
        );

        match AgreementPayment::new(&msg.agreement) {
            Ok(activity) => {
                self.agreements.insert(msg.agreement.agreement_id.clone(), activity);
                Ok(())
            }
            Err(error) => {
                log::error!("Failed to create payment model for agreement [{}]. Error: {}",
                    &msg.agreement.agreement_id,
                    error);
                Err(error)
            }
        }
    }


    fn update_debit_note(&mut self, agreement_id: &str, activity_id: &str, debit_note_id: &Option<String>) -> Result<()> {
        let mut activity = self.agreements
            .get_mut(agreement_id)
            .ok_or(anyhow!("Can't find agreement [{}].", agreement_id))?
            .activities
            .get_mut(activity_id)
            .ok_or(anyhow!("Can't find activity [{}] for agreement [{}].", activity_id, agreement_id))?;

        if let ActivityPayment::Running {..} = activity {
            let new_activity = ActivityPayment::Running{last_debit_note: debit_note_id.clone(), activity_id: activity_id.to_string() };
            *activity = new_activity;
            Ok(())
        }
        else {
            Err(anyhow!("Can't update debit note id for finalized activity."))
        }
    }

    async fn compute_cost(
        payment_model: Arc<Box<dyn PaymentModel>>,
        provider_context: Arc<ProviderCtx>,
        activity_id: String
    ) -> Result<(BigDecimal, Vec<f64>)> {
        let activity_api = provider_context.activity_api.clone();

        // let usage = activity_api.get_activity_usage(&activity_id).await?
        //     .current_usage
        //     .ok_or(anyhow!("Can't query usage for activity [{}].", &activity_id))?;
        let usage = vec![1.0, 1.0];
        let cost = payment_model.compute_cost(&usage)?;

        Ok((cost, usage))
    }

    async fn send_debit_note(
        payment_model: Arc<Box<dyn PaymentModel>>,
        provider_context: Arc<ProviderCtx>,
        mut msg: UpdateCost,
    ) -> Result<UpdateCost> {
        let (cost, usage) = Self::compute_cost(
            payment_model.clone(),
            provider_context.clone(),
            msg.activity_id.clone()
        ).await?;

        log::info!("Current cost for activity [{}]: {}.", &msg.activity_id, &cost);

        let debit_note = NewDebitNote {
            agreement_id: msg.agreement_id.clone(),
            activity_id: Some(msg.activity_id.clone()),
            total_amount_due: cost,
            usage_counter_vector: Some(json!(usage)),
            credit_account_id: provider_context.creadit_account.clone(),
            payment_platform: None,
            payment_due_date: None
        };

        log::debug!("Creating debit note {}.", serde_json::to_string(&debit_note)?);

        let payment_api = provider_context.payment_api.clone();
        let debit_note = payment_api.issue_debit_note(&debit_note).await
            .map_err(|error| anyhow!("Failed to issue debit note for activity [{}]. {}", &msg.activity_id, error))?;

        log::debug!("Sending debit note [{}] for activity [{}].", &debit_note.debit_note_id, &msg.activity_id);
        payment_api.send_debit_note(&debit_note.debit_note_id).await
            .map_err(|error| anyhow!("Failed to send debit note [{}] for activity [{}]. {}", &debit_note.debit_note_id, &msg.activity_id, error))?;

        log::info!("Debit note [{}] for activity [{}] sent.", &debit_note.debit_note_id, &msg.activity_id);

        msg.last_debit_note = Some(debit_note.debit_note_id);
        Ok(msg)
    }

//    async fn send_invoice(
//        payment_model: Arc<Box<dyn PaymentModel>>,
//        provider_context: Arc<ProviderCtx>,
//        activities: Vec<String>,
//        agreement_id: String,
//    ) -> Result<()> {
//        let (cost, usage) = Self::compute_cost(payment_model.clone(), provider_context.clone(), activity_id.clone()).await?;
//
//        log::info!("Final cost for agreement [{}]: {}.", &agreement_id, &cost);
//
//        let invoice = NewInvoice {
//            agreement_id,
//            activity_ids: Some(activities),
//            total_amount_due: cost,
//            usage_counter_vector: Some(json!(usage)),
//            credit_account_id: provider_context.creadit_account.clone(),
//            payment_platform: None,
//        };
//
//        let payment_api = provider_context.payment_api.clone();
//        let debit_note = payment_api.issue_debit_note(&debit_note).await
//            .map_err(|error| anyhow!("Failed to issue debit note for activity [{}]. {}", &activity_id, error))?;
//    }
}

forward_actix_handler!(Payments, AgreementApproved, on_signed_agreement);

impl Handler<ActivityCreated> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityCreated, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.agreement_id) {
            log::info!("Payments - activity {} created. Start computing costs.", &msg.activity_id);

            let msg = UpdateCost{
                agreement_id: msg.agreement_id.clone(),
                activity_id: msg.activity_id.clone(),
                last_debit_note: None,
            };

            // Add activity to list and send debit note after update_interval.
            agreement.activity_created(&msg.activity_id);
            ctx.notify_later(msg, agreement.update_interval);

            ActorResponse::reply(Ok(()))
        }
        else {
            ActorResponse::reply(Err(anyhow!("Agreement [{}] wasn't registered.", &msg.agreement_id)))
        }
    }
}

impl Handler<ActivityDestroyed> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityDestroyed, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.agreement_id) {
            if let Some(activity) = agreement.activities.get_mut(&msg.activity_id) {
                if let ActivityPayment::Running {..} = activity {

                    //TODO: Send invoice

                    log::info!("Payments - activity {} destroyed.", &msg.agreement_id);
                    return ActorResponse::reply(Ok(()))
                }
                else {
                    log::warn!("Trying to destroy already destroyed activity [{}] for agreement [{}].", &msg.activity_id, &msg.agreement_id);
                    return ActorResponse::reply(Err(anyhow!("")))
                }
            }
        }

        log::warn!("Can't find activity [{}] and agreement [{}].", &msg.activity_id, &msg.agreement_id);
        ActorResponse::reply(Err(anyhow!("")))
    }
}

impl Handler<UpdateCost> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: UpdateCost, ctx: &mut Context<Self>) -> Self::Result {
        let agreement = match self.agreements.get(&msg.agreement_id) {
            Some(agreement) => agreement,
            None => {
                log::warn!("Not my activity - agreement [{}].", &msg.agreement_id);
                return ActorResponse::reply(Err(anyhow!("")))
            }
        };

        if let Some(activity) = agreement.activities.get(&msg.activity_id) {
            if let ActivityPayment::Running {..} = activity {
                let payment_model = agreement.payment_model.clone();
                let context= self.context.clone();
                let msg = msg.clone();
                let update_interval= agreement.update_interval;

                return ActorResponse::r#async(async move {
                    Self::send_debit_note(payment_model, context, msg).await
                }
                .into_actor(self)
                .map(move |result, sself, ctx| {
                    match result {
                        Ok(msg) => {
                            // msg contains updated debit_note_id.
                            sself.update_debit_note(&msg.agreement_id, &msg.activity_id, &msg.last_debit_note)?;
                            ctx.notify_later(msg, update_interval);
                            Ok(())
                        },
                        Err(error) => {
                            log::error!("{}", error);
                            return Err(error);
                        }
                    }
                }));
            }
            else {
                // Note: we don't send here new UpdateCost message, what stops further updates.
                log::info!("Stopped sending debit notes, because activity {} was destroyed.", &msg.activity_id);
                return ActorResponse::reply(Ok(()))
            }
        }
        return ActorResponse::reply(Err(anyhow!("We shouldn't be here. Activity [{}], not found.", &msg.activity_id)))
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
        let activity = ActivityPayment::Running{activity_id: activity_id.to_string(), last_debit_note: None};
        self.activities.insert(activity_id.to_string(), activity);
    }
}
