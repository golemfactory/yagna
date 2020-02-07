use ya_persistence::executor::DbExecutor;

type Result<T> = std::result::Result<T, diesel::result::Error>;

mod agreement;
pub use agreement::AgreementDao;

pub fn init(db: &DbExecutor) -> anyhow::Result<()> {
    db.apply_migration(crate::db::migrations::run_with_output)
}
