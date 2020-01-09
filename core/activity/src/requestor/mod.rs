use crate::dao::{ActivityDao, AgreementDao, NotFoundAsOption};
use crate::error::Error;
use futures::lock::Mutex;
use ya_persistence::executor::DbExecutor;
use ya_persistence::models::Agreement;

pub mod control;
pub mod state;

async fn get_agreement(
    db_executor: &Mutex<DbExecutor<Error>>,
    activity_id: &str,
) -> Result<Agreement, Error> {
    let conn = db_executor.lock().await.conn()?;
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
