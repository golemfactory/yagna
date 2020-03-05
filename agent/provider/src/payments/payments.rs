use actix::prelude::*;
use anyhow::{Result, anyhow, Error};
use log;
use std::sync::Arc;
use std::collections::HashMap;
use std::time::Duration;

use crate::market::provider_market::AgreementSigned;
use crate::execution::{ActivityCreated, ActivityDestroyed};
use super::model::{PaymentModel, PaymentDescription};

use ya_client::activity::ActivityProviderApi;
use ya_client::payment::provider::ProviderApi;
use ya_utils_actix::actix_handler::{ResultTypeGetter};
use ya_utils_actix::forward_actix_handler;
use ya_model::market::Agreement;
use crate::payments::factory::PaymentModelFactory;


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
    Running,
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
            if activity.state == ActivityState::Running {
                let msg = UpdateCost{agreement_id: activity.agreement_id.clone()};
                ctx.address().do_send(msg);
            }
        }
    }
}

forward_actix_handler!(Payments, AgreementSigned, on_signed_agreement);

impl Handler<ActivityCreated> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityCreated, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(activity) = self.agreements.get_mut(&msg.agreement_id) {
            log::info!("Payments - activity {} created. Start computing costs.", &msg.agreement_id);

            activity.change_to_running();
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
            let error = format!("Not my activity - agreement [{}].", &msg.agreement_id);
            log::warn!("{}", error);
            ActorResponse::reply(Err(Error::msg(error)))
        }
    }
}

impl Handler<UpdateCost> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: UpdateCost, ctx: &mut Context<Self>) -> Self::Result {
        ActorResponse::reply(Ok(()))
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

    pub fn change_to_running(&mut self) {
        self.state = ActivityState::Running;
    }
}
