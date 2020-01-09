use crate::dao::{ActivityDao, AgreementDao, NotFoundAsOption};
use crate::error::Error;
use crate::{ACTIVITY_SERVICE_ID, NET_SERVICE_ID};
use futures::lock::Mutex;
use std::sync::Arc;
use ya_persistence::executor::DbExecutor;
use ya_persistence::models::Agreement;

pub mod control;
pub mod state;

fn uri(provider_id: &str, cmd: &str) -> String {
    format!(
        "/{}/{}/{}/{}",
        NET_SERVICE_ID, provider_id, ACTIVITY_SERVICE_ID, cmd
    )
}

async fn get_agreement(
    db: &Arc<Mutex<DbExecutor<Error>>>,
    activity_id: &str,
) -> Result<Agreement, Error> {
    let conn = db_conn!(db)?;
    let agreement_id = ActivityDao::new(&conn)
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
