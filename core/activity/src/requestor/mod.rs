use crate::dao::{ActivityDao, AgreementDao, NotFoundAsOption};
use crate::error::Error;

use ya_core_model::activity::ACTIVITY_SERVICE_ID;
use ya_persistence::{executor::ConnType, models::Agreement};
use ya_service_api::constants::NET_SERVICE_ID;


pub mod control;
pub mod state;

#[inline(always)]
fn provider_activity_uri(provider_id: &str) -> String {
    format!("{}/{}/{}", NET_SERVICE_ID, provider_id, ACTIVITY_SERVICE_ID)
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

fn get_agreement(conn: &ConnType, activity_id: &str) -> Result<Agreement, Error> {
    let agreement_id = ActivityDao::new(conn)
        .get_agreement_id(activity_id)
        .not_found_as_option()
        .map_err(Error::from)?
        .ok_or(Error::NotFound)?;

    AgreementDao::new(&conn)
        .get(&agreement_id)
        .not_found_as_option()
        .map_err(Error::from)?
        .ok_or(Error::NotFound)
}
