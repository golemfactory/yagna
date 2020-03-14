use ya_core_model::activity;
use ya_model::market::Agreement;

use crate::error::Error;

pub mod control;
pub mod state;

#[inline(always)]
fn provider_activity_service_id(agreement: &Agreement) -> Result<String, Error> {
    Ok(ya_net::remote_service(
        agreement.offer.provider_id()?,
        activity::SERVICE_ID,
    ))
}
