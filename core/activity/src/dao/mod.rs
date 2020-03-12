mod activity;
mod activity_state;
mod activity_usage;
mod event;

pub use activity::ActivityDao;
pub use activity_state::ActivityStateDao;
pub use activity_usage::ActivityUsageDao;
pub use event::{Event, EventDao};
use thiserror::Error;

type Result<T> = std::result::Result<T, DaoError>;

no_arg_sql_function!(last_insert_rowid, diesel::sql_types::Integer);

#[derive(Error, Debug)]
pub enum DaoError {
    #[error("Not found")]
    NotFound,
    #[error("Diesel error: {0}")]
    DieselError(String),
    #[error("Tokio error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("R2D2 error: {0}")]
    R2D2Error(#[from] r2d2::Error),
}

impl From<diesel::result::Error> for DaoError {
    fn from(error: diesel::result::Error) -> Self {
        match &error {
            diesel::result::Error::NotFound => DaoError::NotFound,
            _ => DaoError::DieselError(format!("{:?}", error)),
        }
    }
}
