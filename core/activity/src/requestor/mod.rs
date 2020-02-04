use ya_core_model::activity::SERVICE_ID;
use ya_model::market::Agreement;
use ya_persistence::executor::ConnType;
use ya_service_api::constants::{NET_SERVICE_ID, PRIVATE_SERVICE};

use crate::dao::ActivityDao;
use crate::error::Error;

pub mod control;
pub mod state;

#[inline(always)]
fn provider_activity_service_id(agreement: &Agreement) -> Result<String, Error> {
    let provider_id = agreement
        .offer
        .provider_id
        .as_ref()
        .ok_or(Error::BadRequest("no provider id".into()))?;

    Ok(format!(
        "{}{}/{}{}",
        PRIVATE_SERVICE, NET_SERVICE_ID, provider_id, SERVICE_ID
    ))
}

fn missing_activity_err(conn: &ConnType, activity_id: &str) -> Result<(), Error> {
    let exists = ActivityDao::new(conn)
        .exists(activity_id)
        .map_err(Error::from)?;
    match exists {
        true => Ok(()),
        false => Err(Error::NotFound),
    }
}
