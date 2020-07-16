use crate::error::DbError;
use ya_persistence::executor::DbExecutor;

pub mod payment;
pub mod transaction;

pub type DbResult<T> = Result<T, DbError>;

pub async fn init(db: &DbExecutor) -> anyhow::Result<()> {
    db.apply_migration(crate::migrations::run_with_output)
}
