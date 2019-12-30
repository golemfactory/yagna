use crate::dao::{ActivityDao, Agreement, AgreementDao, InnerIntoOption};
use crate::db::DbExecutor;
use crate::error::Error;
use futures::lock::Mutex;

pub mod control;
pub mod state;

async fn get_agreement(
    db_executor: &Mutex<DbExecutor>,
    activity_id: &str,
) -> Result<Agreement, Error> {
    let conn = db_executor.lock().await.conn()?;
    let agreement_id = ActivityDao::new(&conn)
        .get_agreement_id(activity_id)
        .inner_into_option()
        .map_err(Error::from)?
        .ok_or(Error::NotFound)?;

    AgreementDao::new(&conn)
        .get(&agreement_id)
        .inner_into_option()
        .map_err(Error::from)?
        .ok_or(Error::NotFound)
}
