use ya_persistence::executor::DbExecutor;

mod agreement;
pub use agreement::AgreementDao;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("DB connection error: {0}")]
    Db(#[from] r2d2::Error),
    #[error("DAO error: {0}")]
    Dao(#[from] diesel::result::Error),
    #[error("task: {0}")]
    RuntimeError(#[from] tokio::task::JoinError),
}

type Result<T> = std::result::Result<T, Error>;

pub fn init(db: &DbExecutor) -> anyhow::Result<()> {
    db.apply_migration(crate::db::migrations::run_with_output)
}
