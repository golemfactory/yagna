use ya_persistence::executor::DbExecutor;

pub mod payment;
pub mod transaction;

#[allow(unused)]
pub async fn init(db: &DbExecutor) -> anyhow::Result<()> {
    db.apply_migration(crate::migrations::run_with_output)
}
