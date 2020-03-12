use ya_core_model::{activity, net};
use ya_model::market::Agreement;

use crate::error::Error;

pub mod control;
pub mod state;

#[inline(always)]
fn remote_service(node_id: &str) -> String {
    format!("{}{}/{}", net::PRIVATE_PREFIX, net::SERVICE_ID, node_id)
}

#[inline(always)]
fn provider_activity_service_id(agreement: &Agreement) -> Result<String, Error> {
    Ok(format!(
        "{}{}",
        remote_service(agreement.provider_id()?),
        activity::SERVICE_ID
    ))
}

    Ok(format!(
    ))
}
