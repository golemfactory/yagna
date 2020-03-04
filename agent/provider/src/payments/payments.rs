use actix::prelude::*;
use anyhow::Result;
use log;
use std::sync::Arc;

use crate::market::provider_market::AgreementSigned;
use ya_client::activity::ActivityProviderApi;
use ya_client::payment::provider::ProviderApi;
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::forward_actix_handler;




// =========================================== //
// Payments implementation
// =========================================== //

/// Computes charges for tasks execution.
/// Sends payments events to requestor through payment API.
pub struct Payments {
    activity_api: Arc<ActivityProviderApi>,
    payment_api: Arc<ProviderApi>,
}

impl Payments {
    pub fn new(activity_api: ActivityProviderApi, payment_api: ProviderApi) -> Payments {
        Payments{
            activity_api: Arc::new(activity_api),
            payment_api: Arc::new(payment_api)
        }
    }

    pub fn on_signed_agreement(&mut self, msg: AgreementSigned) -> Result<()> {
        log::info!(
            "Payments got signed agreement [{}] for processing.",
            &msg.agreement.agreement_id
        );
        Ok(())
    }


}

forward_actix_handler!(Payments, AgreementSigned, on_signed_agreement);

impl Actor for Payments {
    type Context = Context<Self>;
}


