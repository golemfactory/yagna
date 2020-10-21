mod activity;
mod activity_credentials;
mod activity_state;
mod activity_usage;
mod event;

pub use activity::ActivityDao;
pub use activity_credentials::ActivityCredentialsDao;
pub use activity_state::ActivityStateDao;
pub use activity_usage::ActivityUsageDao;
pub use event::{Event, EventDao};
use thiserror::Error;

type Result<T> = std::result::Result<T, DaoError>;

no_arg_sql_function!(last_insert_rowid, diesel::sql_types::Integer);

#[derive(Error, Debug)]
pub enum DaoError {
    #[error("Diesel error: {0}")]
    DieselError(#[from] diesel::result::Error),
    #[error("Tokio error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("R2D2 error: {0}")]
    R2D2Error(#[from] r2d2::Error),
    #[error("Serde Json error: {0}")]
    SerdeJsonError(#[from] serde_json::error::Error),
    #[error("Not found: {0}")]
    NotFound(String),
}

impl From<ya_persistence::executor::Error> for DaoError {
    fn from(err: ya_persistence::executor::Error) -> Self {
        match err {
            ya_persistence::executor::Error::DieselError(e) => e.into(),
            ya_persistence::executor::Error::PoolError(e) => e.into(),
            ya_persistence::executor::Error::RuntimeError(e) => e.into(),
            ya_persistence::executor::Error::SerdeJsonError(e) => e.into(),
        }
    }
}
