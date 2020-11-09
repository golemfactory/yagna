use crate::dao::error::DbError;

pub mod payment;
pub mod transaction;
pub mod error;


pub use ya_persistence::executor::DbExecutor;

pub type DbResult<T> = Result<T, DbError>;

pub async fn init(db: &DbExecutor) -> anyhow::Result<()> {
    db.apply_migration(crate::db::migrations::run_with_output)
}
