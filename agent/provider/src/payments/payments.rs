use actix::prelude::*;
use anyhow::{Result, anyhow, Error};
use log;
use serde_json::json;
use std::sync::Arc;
use std::collections::HashMap;
use std::time::Duration;
use futures_util::FutureExt;

use crate::market::provider_market::AgreementSigned;
use crate::execution::{ActivityCreated, ActivityDestroyed};
use super::model::{PaymentModel, PaymentDescription};
use crate::payments::factory::PaymentModelFactory;

use ya_client::activity::ActivityProviderApi;
use ya_client::payment::provider::ProviderApi;
use ya_utils_actix::actix_handler::{ResultTypeGetter};
use ya_utils_actix::forward_actix_handler;
use ya_model::market::Agreement;
use ya_model::payment::NewDebitNote;


const UPDATE_COST_INTERVAL_MILLIS: u64 = 10000;

// =========================================== //
// Internal messages
// =========================================== //

/// Checks activity usage counters and updates service
/// cost. Sends debit note to requestor.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct UpdateCost {
    pub agreement_id: String,
}

// =========================================== //
// Payments implementation
// =========================================== //

#[derive(PartialEq)]
enum ActivityState {
    AgreementSigned,
    Running {
        activity_id: String,
    },
}

struct ActivityPayment {
    agreement_id: String,
    payment_model: Arc<Box<dyn PaymentModel>>,
    state: ActivityState,
}

/// Computes charges for tasks execution.
/// Sends payments events to requestor through payment API.
pub struct Payments {
    activity_api: Arc<ActivityProviderApi>,
    payment_api: Arc<ProviderApi>,

    agreements: HashMap<String, ActivityPayment>,
}

impl Payments {
    pub fn new(activity_api: ActivityProviderApi, payment_api: ProviderApi) -> Payments {
        Payments{
            activity_api: Arc::new(activity_api),
            payment_api: Arc::new(payment_api),
            agreements: HashMap::new(),
        }
    }

    pub fn on_signed_agreement(&mut self, msg: AgreementSigned) -> Result<()> {
        log::info!(
            "Payments got signed agreement [{}].",
            &msg.agreement.agreement_id
        );

        match ActivityPayment::new(&msg.agreement) {
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

    fn update_all_costs(&mut self, ctx: &mut Context<Self>) {
        for (id, activity) in self.agreements.iter_mut() {
            // Update costs only for agreements, that has created activity.
            if let ActivityState::Running{..} = activity.state {
                let msg = UpdateCost{agreement_id: activity.agreement_id.clone()};
                ctx.address().do_send(msg);
            }
        }
    }

    async fn send_debit_note(
        payment_model: Arc<Box<dyn PaymentModel>>,
        activity_api: Arc<ActivityProviderApi>,
        payment_api: Arc<ProviderApi>,
        activity_id: String,
    ) -> Result<()> {
        let usage = activity_api.get_activity_usage(&activity_id).await?
            .current_usage
            .ok_or(anyhow!("Can't query usage for activity [{}].", &activity_id))?;

        let cost = payment_model.compute_cost(&usage)?;

        let debit_note = NewDebitNote {
            agreement_id: "".to_string(),
            activity_id: Some(activity_id),
            total_amount_due: cost,
            usage_counter_vector: Some(json!(usage)),
            credit_account_id: "".to_string(),
            payment_platform: None,
            payment_due_date: None
        };

        let debit_note = payment_api.issue_debit_note(&debit_note).await?;
        payment_api.send_debit_note(&debit_note.debit_note_id).await?;
        Ok(())
    }
}

forward_actix_handler!(Payments, AgreementSigned, on_signed_agreement);

impl Handler<ActivityCreated> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityCreated, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(activity) = self.agreements.get_mut(&msg.agreement_id) {
            log::info!("Payments - activity {} created. Start computing costs.", &msg.agreement_id);

            activity.activity_created(&msg.activity_id);
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
        if let Some(activity) = self.agreements.remove(&msg.agreement_id) {
            //TODO: Send invoice

            log::info!("Payments - activity {} destroyed.", &msg.agreement_id);
            ActorResponse::reply(Ok(()))
        }
        else {
            log::warn!("Not my activity - agreement [{}].", &msg.agreement_id);
            ActorResponse::reply(Err(anyhow!("")))
        }
    }
}

impl Handler<UpdateCost> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: UpdateCost, ctx: &mut Context<Self>) -> Self::Result {
        let activity = match self.agreements.get(&msg.agreement_id) {
            Some(activity) => activity,
            None => {
                log::warn!("Not my activity - agreement [{}].", &msg.agreement_id);
                return ActorResponse::reply(Err(anyhow!("")))
            }
        };

        if let ActivityState::Running {activity_id} = &activity.state {
            let payment_model = activity.payment_model.clone();
            let activity_api = self.activity_api.clone();
            let payment_api = self.payment_api.clone();
            let activity_id = activity_id.clone();

            let future= async move {
                Self::send_debit_note(payment_model, activity_api, payment_api, activity_id).await
            }.into_actor(self).map(|result, _, _|{
                if let Err(error) = result {
                    log::error!("{}", error);
                    return Err(error);
                };
                Ok(())
            });

            ActorResponse::r#async(future)
        }
        else {
            log::error!("Code error: ActivityState is not 'Running' in UpdateCost function.");
            return ActorResponse::reply(Err(anyhow!("")))
        }
    }
}

impl Actor for Payments {
    type Context = Context<Self>;

    /// Starts cost updater functions.
    fn started(&mut self, context: &mut Context<Self>) {
        IntervalFunc::new(Duration::from_secs(4), Self::update_all_costs)
            .finish()
            .spawn(context);
    }
}

impl ActivityPayment {
    pub fn new(agreement: &Agreement) -> Result<ActivityPayment> {
        let payment_description = PaymentDescription::new(agreement)?;
        let payment_model = PaymentModelFactory::create(payment_description)?;

        Ok(ActivityPayment {
            agreement_id: agreement.agreement_id.clone(),
            state: ActivityState::AgreementSigned,
            payment_model
        })
    }

    pub fn activity_created(&mut self, activity_id: &str) {
        self.state = ActivityState::Running{activity_id: activity_id.to_string() };
    }
}
