use actix::prelude::*;
use anyhow::{Result, Error};
use log;
use std::sync::Arc;

use crate::market::provider_market::AgreementSigned;
use crate::execution::{ActivityCreated, ActivityDestroyed};

use ya_client::activity::ActivityProviderApi;
use ya_client::payment::provider::ProviderApi;
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::forward_actix_handler;
use std::collections::HashMap;
use ya_model::market::Agreement;


// =========================================== //
// Internal messages
// =========================================== //

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct UpdateAgreementPayment;

// =========================================== //
// Payments implementation
// =========================================== //

/// Computes charges for tasks execution.
/// Sends payments events to requestor through payment API.
pub struct Payments {
    activity_api: Arc<ActivityProviderApi>,
    payment_api: Arc<ProviderApi>,

    agreements: HashMap<String, Agreement>,
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
            "Payments got signed agreement [{}] for processing.",
            &msg.agreement.agreement_id
        );
        self.agreements.insert(msg.agreement.agreement_id.clone(), msg.agreement);
        Ok(())
    }

    fn on_activity_destroyed(&mut self, msg: ActivityDestroyed) -> Result<()> {
        log::info!("Payments - activity {} destroyed.", &msg.agreement_id);
        Ok(())
    }
}

forward_actix_handler!(Payments, AgreementSigned, on_signed_agreement);
forward_actix_handler!(Payments, ActivityDestroyed, on_activity_destroyed);

impl Handler<ActivityCreated> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityCreated, _ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get(&msg.agreement_id) {
            log::info!("Payments - activity {} created.", &msg.agreement_id);



            ActorResponse::reply(Ok(()))
        }
        else {
            ActorResponse::reply(Err(Error::msg(format!("Agreement wasn't registered."))))
        }
    }
}

impl Actor for Payments {
    type Context = Context<Self>;
}


